use super::envelope::Envelope;



pub(super) enum PulseId {
    Pulse1,
    Pulse2,
}

struct Pulse {
    // 组件

}

impl Pulse {
    pub(super) fn new(pulse_id: PulseId) -> Self {
        todo!()
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