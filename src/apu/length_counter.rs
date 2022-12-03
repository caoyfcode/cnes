pub(super) struct LengthCounter {
    counter: u8,
    halt_flag: bool,
}

impl LengthCounter {
    const LENGTH_TABLE: [u8; 32] = [
        10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96,
        22, 192, 24, 72, 26, 16, 28, 32, 30,
    ];

    pub(super) fn new() -> Self {
        Self {
            counter: 0,
            halt_flag: false,
        }
    }

    pub(super) fn counter(&self) -> u8 {
        self.counter
    }

    /// 将 counter 置为 0
    pub(super) fn clear_counter(&mut self) {
        self.counter = 0;
    }

    pub(super) fn set_halt_flag(&mut self, halt_flag: bool) {
        self.halt_flag = halt_flag;
    }

    /// 必须在对应通道 enable flag 为 true 时调用
    pub(super) fn load(&mut self, load_val: u8) {
        self.counter = Self::LENGTH_TABLE[load_val as usize];
    }

    pub(super) fn on_half_frame(&mut self) {
        if self.counter == 0 || self.halt_flag {
            return;
        }
        self.counter -= 1;
    }
}
