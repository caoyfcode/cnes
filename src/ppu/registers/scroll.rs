pub(in crate::ppu) struct ScrollRegister {
    pub scroll_x: u8,
    pub scroll_y: u8,
    write_x: bool,
}

impl ScrollRegister {
    pub fn new() -> Self {
        ScrollRegister {
            scroll_x: 0,
            scroll_y: 0,
            write_x: true,
        }
    }

    pub fn write(&mut self, data: u8) {
        if self.write_x {
            self.scroll_x = data;
        } else {
            self.scroll_y = data;
        }

        self.write_x = !self.write_x;
    }

    pub fn reset_latch(&mut self) {
        self.write_x = true;
    }
}