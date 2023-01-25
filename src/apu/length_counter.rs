pub(super) struct LengthCounter {
    enabled_flag: bool,
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
            enabled_flag: false,
            counter: 0,
            halt_flag: false,
        }
    }

    pub(super) fn counter(&self) -> u8 {
        self.counter
    }

    // $4000/$4004/400C, $4008 usage
    pub(super) fn set_halt_flag(&mut self, halt_flag: bool) {
        self.halt_flag = halt_flag;
    }

    // $4003/$4007/400B/$400F usage
    pub(super) fn load_if_enabled_flag(&mut self, load_val: u8) {
        if self.enabled_flag {
            self.counter = Self::LENGTH_TABLE[load_val as usize];
        }
    }

    // $4015 write ---D NT21 Enable DMC (D), noise (N), triangle (T), and pulse channels (2/1)
    pub(super) fn set_enabled_flag(&mut self, enabled: bool) {
        self.enabled_flag = enabled;
        if !enabled {
            self.counter = 0;
        }
    }

    pub(super) fn on_half_frame(&mut self) {
        if self.counter == 0 || self.halt_flag {
            return;
        }
        self.counter -= 1;
    }
}
