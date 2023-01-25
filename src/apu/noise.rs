use super::{envelope::Envelope, length_counter::LengthCounter};


pub(super) struct Noise {
    envelope: Envelope,
    timer_reset: u16,
    timer_counter: u16,
    shift_register: u16, // 15-bit shift register
    mode_flag: bool, // 决定反馈函数(0: r[0] xor r[1]; 1: r[0] xor r[6])
    length_counter: LengthCounter,
}

impl Noise {
    /// 16 种周期(设置timer reset时要减去1)
    ///
    /// 使用 NTSC 标准, 见 https://www.nesdev.org/wiki/APU_Noise.
    const TIMER_PERIOD_TABLE: [u16; 16] = [
        4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1016, 2034, 4068
    ];

    pub(super) fn new() -> Self {
        Self {
            envelope: Envelope::new(),
            timer_reset: 0,
            timer_counter: 0,
            shift_register: 1,
            mode_flag: false,
            length_counter: LengthCounter::new(),
        }
    }

    /// $400C  --lc.vvvv  Length counter halt, constant volume/envelope flag, and volume/envelope divider period (write)
    /// - L envelope loop/length counter halt
    /// - C constant volume
    /// - V volume/envelope (V)
    pub(super) fn write_ctrl(&mut self, data: u8) {
        let loop_and_halt = data & 0b0010_0000 == 0b0010_0000;
        let is_constant = data & 0b0001_0000 == 0b0001_0000;
        let volume_and_envelope = data & 0b1111;
        self.envelope.set_loop_flag(loop_and_halt);
        self.envelope.set_constant_volume_flag(is_constant);
        self.envelope.set_constant_volume(volume_and_envelope);
        self.length_counter.set_halt_flag(loop_and_halt);
    }

    /// $400E  M---.PPPP  Mode and period (write)
    pub(super) fn write_mode_and_period(&mut self, data: u8) {
        self.mode_flag = data & 0b1000_0000 == 0b1000_0000;
        self.timer_reset = Self::TIMER_PERIOD_TABLE[(data & 0b1111) as usize] - 1;
    }

    /// $400F  llll.l---  Length counter load and envelope restart (write)
    pub(super) fn write_length_counter_load(&mut self, data: u8) {
        self.length_counter.load_if_enabled_flag(data >> 3);
        self.envelope.set_start_flag();
    }

    /// Status ($4015)
    pub(super) fn set_enabled_flag(&mut self, enabled: bool) {
        self.length_counter.set_enabled_flag(enabled);
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
    }

    pub(super) fn on_apu_clock(&mut self) {
        // timer 滴答
        if self.timer_counter != 0 {
            self.timer_counter -= 1;
        } else {
            self.timer_counter = self.timer_reset;
            let bit1 = self.shift_register & 1; // r[0]
            let bit2 = match self.mode_flag {
                false => (self.shift_register >> 1) & 1, // r[1]
                true => (self.shift_register >> 6) & 1, // r[6]
            };
            let feedback = bit1 ^ bit2;
            self.shift_register = (self.shift_register >> 1) | (feedback << 14);
        }
    }

    pub(super) fn output(&self) -> u8 {
        if self.shift_register & 1 == 1 || self.length_counter.counter() == 0 {
            0
        } else {
            self.envelope.output()
        }
    }
}