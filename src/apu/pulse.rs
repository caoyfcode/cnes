use super::{envelope::Envelope, length_counter::LengthCounter};



pub(super) enum PulseId {
    Pulse1,
    Pulse2,
}


/// 方波通道
pub(super) struct Pulse {
    enabled_flag: bool, // 将 enabled flag 清零将导致 length counter 清零
    envelope: Envelope,
    sweep: Sweep,
    timer_reset: u16, // 11bit timer, 用于控制频率
    timer_counter: u16,
    sequencer_duty_type: usize, // 0..=3, 四种不同占空比
    sequencer_step: usize, // 0..=7
    length_counter: LengthCounter,
}

impl Pulse {
    const DUTY_TABLE: [[u8; 8]; 4] = [ // 不同占空比的波形
        [0, 1, 0, 0, 0, 0, 0, 0], // 12.5%
        [0, 1, 1, 0, 0, 0, 0, 0], // 25%
        [0, 1, 1, 1, 1, 0, 0, 0], // 50%
        [1, 0, 0, 1, 1, 1, 1, 1]  // 25 negated
    ];

    pub(super) fn new(pulse_id: PulseId) -> Self {
        Self {
            enabled_flag: true,
            envelope: Envelope::new(),
            sweep: Sweep::new(pulse_id),
            timer_reset: 0,
            timer_counter: 0,
            sequencer_duty_type: 0,
            sequencer_step: 0,
            length_counter: LengthCounter::new(),
        }
    }

    /// $4000/$4004 DDLC VVVV
    /// - D Duty
    /// - L envelope loop/length counter halt
    /// - C constant volume
    /// - V volume/envelope (V)
    ///
    /// The duty cycle is changed (see table below), but the sequencer's current position isn't affected.
    pub(super) fn write_ctrl(&mut self, data: u8) {
        let duty = data >> 6;
        let loop_and_halt = data & 0b0010_0000 == 0b0010_0000;
        let is_constant = data & 0b0001_0000 == 0b0001_0000;
        let volume_and_envelope = data & 0b1111;
        self.sequencer_duty_type = duty as usize;
        self.envelope.set_loop_flag(loop_and_halt);
        self.envelope.set_constant_volume_flag(is_constant);
        self.envelope.set_constant_volume(volume_and_envelope);
        self.length_counter.set_halt_flag(loop_and_halt);
    }

    /// $4001/$4005
    pub(super) fn write_sweep(&mut self, data: u8) {
        self.sweep.write(data);
    }

    /// $4002/$4006 timer low 8 bits
    pub(super) fn write_timer_lo(&mut self, data: u8) {
        self.timer_reset = (self.timer_reset & 0xff00) | (data as u16);
    }

    /// $4003/$4007 LLLL LTTT Length counter load (L), timer high (T)
    ///
    /// The sequencer is immediately restarted at the first value of the current sequence. The envelope is also restarted.
    pub(super) fn write_length_load_and_timer_hi(&mut self, data: u8) {
        self.timer_reset = (((data & 0b111) as u16) << 8) | (self.timer_reset & 0xff);
        self.timer_counter = self.timer_reset;
        self.sequencer_step = 0;
        self.envelope.set_start_flag();
        if self.enabled_flag {
            self.length_counter.load(data >> 3);
        }
    }

    /// Status ($4015)
    pub(super) fn set_enabled_flag(&mut self, enabled: bool) {
        self.enabled_flag = enabled;
        if !enabled {
            self.length_counter.clear_counter();
        }
    }

    /// Status ($4015) read
    pub(super) fn length_counter(&self) -> u8 {
        self.length_counter.counter()
    }

    pub(super) fn on_quarter_frame(&mut self) {
        self.envelope.on_quarter_frame();
    }

    pub(super) fn on_half_frame(&mut self) {
        self.length_counter.on_half_frame();
        self.sweep.on_half_frame(&mut self.timer_reset);
    }

    pub(super) fn on_apu_clock(&mut self) {
        // timer 滴答
        if self.timer_counter != 0 {
            self.timer_counter -= 1;
        } else {
            self.timer_counter = self.timer_reset;
            self.sequencer_step = (self.sequencer_step + 1) % 8;
        }
    }

    pub(super) fn output(&self) -> u8 {
        if Self::DUTY_TABLE[self.sequencer_duty_type][self.sequencer_step] != 0
            && !self.sweep.forcing_silence(self.timer_reset)
            && self.length_counter.counter() != 0
        {
            self.envelope.output()
        } else {
            0
        }
    }
}

/// Sweep 单元, 通过控制 pulse 通道 timer 的重置值来控制 pulse 的频率.
struct Sweep {
    // 组件
    divider_reset: u8, // 3bit, divider 重置值
    divider_counter: u8,
    shift: u8, // 3bit, 将 timer_reset 右移 shift 位得到 change amount
    reload_flag: bool, // divider reload flag
    enable_flag: bool, // 是否要改变 timer reset
    negate_flag: bool, // change amount 加还是减
    // 状态信息
    pulse_id: PulseId,
}

impl Sweep {
    fn new(pulse_id: PulseId) -> Self {
        Self {
            divider_reset: 1,
            divider_counter: 0,
            shift: 0,
            reload_flag: false,
            enable_flag: false,
            negate_flag: false,
            pulse_id,
        }
    }

    fn write(&mut self, data: u8) {
        self.enable_flag = data & 0b1000_0000 == 0b1000_0000;
        self.divider_reset = (data & 0b0111_0000) >> 4;
        self.negate_flag = data & 0b0000_1000 == 0b0000_1000;
        self.shift = data & 0b111;
        self.reload_flag = true;
    }

    fn on_half_frame(&mut self, timer_reset: &mut u16) {
        // 适时改变 timer_reset
        if self.divider_counter == 0 && self.enable_flag && !self.forcing_silence(*timer_reset) {
            let change_amount = *timer_reset >> self.shift;
            if !self.negate_flag {
                *timer_reset += change_amount;
            } else {
                *timer_reset -= match self.pulse_id {
                    PulseId::Pulse1 => change_amount + 1,
                    PulseId::Pulse2 => change_amount
                }
            }
        }
        if self.divider_counter == 0 || self.reload_flag {
            self.divider_counter = self.divider_reset;
            self.reload_flag = false;
        } else {
            self.divider_counter -= 1;
        }
    }

    // 是否强制静音
    fn forcing_silence(&self, timer_reset: u16) -> bool {
        let change_amount = timer_reset >> self.shift;
        timer_reset < 8 || (!self.negate_flag && timer_reset + change_amount > 0x7ff)
    }
}