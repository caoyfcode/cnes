
/// 用于生成包络:
/// - 递减的锯齿包络, 是否循环可选
/// - 恒定的常数
pub(super) struct Envelope {
    // 控制位
    start_flag: bool,
    loop_flag: bool,
    constant_volume_flag: bool,
    // 其余组成部分:
    divider_counter: u8, // 用于控制递减的包络的周期
    decay_level_counter: u8,
    // 状态信息
    constant_volume: u8, // 4 bit, constant volume or the reload value for divider
}

impl Envelope {
    const DECAY_LEVEL_RESET: u8 = 15;

    pub(super) fn new() -> Self {
        Self {
            start_flag: true,
            loop_flag: false,
            constant_volume_flag: true,
            divider_counter: 0,
            decay_level_counter: Self::DECAY_LEVEL_RESET,
            constant_volume: 0,
        }
    }

    pub(super) fn set_start_flag(&mut self) {
        self.start_flag = true;
    }

    pub(super) fn write(&mut self, data: u8) {
        self.loop_flag = data & 0b0010_0000 == 0b0010_0000;
        self.constant_volume_flag = data & 0b0001_0000 == 0b0001_0000;
        self.constant_volume = data & 0b1111;
    }

    pub(super) fn on_quarter_frame(&mut self) {
        if self.start_flag {
            self.start_flag = false;
            self.divider_counter = self.constant_volume;
            self.decay_level_counter = Self::DECAY_LEVEL_RESET;
            return;
        }
        // start_flag is 0
        if self.divider_counter != 0 {
            self.divider_counter -= 1;
            return;
        }
        // divider is 0
        self.divider_counter = self.constant_volume;
        if self.decay_level_counter !=0 {
            self.decay_level_counter -= 1;
        } else if self.loop_flag {
            self.decay_level_counter = Self::DECAY_LEVEL_RESET;
        }
    }

    pub(super) fn output(&self) -> u8 {
        if self.constant_volume_flag {
            self.constant_volume
        } else {
            self.decay_level_counter
        }
    }

}