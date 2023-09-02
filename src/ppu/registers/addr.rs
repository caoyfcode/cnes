pub(in crate::ppu) struct AddrRegister {
    hi: u8,
    lo: u8,
    write_hi: bool,
}

impl AddrRegister {
    pub fn new() -> Self {
        AddrRegister {
            hi: 0,
            lo: 0,
            write_hi: true,
        }
    }

    pub fn write(&mut self, data: u8) {
        if self.write_hi {
            self.hi = data;
        } else {
            self.lo = data;
        }

        if self.get() > 0x3fff { // mirror down addr above 0x3fff
            self.set(self.get()  & 0b0011_1111_1111_1111);
        }

        self.write_hi = !self.write_hi;
    }

    pub fn increment(&mut self, inc: u8) {
        let old = self.lo;
        self.lo = self.lo.wrapping_add(inc);
        if old > self.lo {
            self.hi = self.hi.wrapping_add(1);
        }

        if self.get() > 0x3fff { // mirror down addr above 0x3fff
            self.set(self.get()  & 0b0011_1111_1111_1111);
        }
    }

    pub fn reset_latch(&mut self) {
        self.write_hi = true;
    }

    pub fn get(&self) -> u16 {
        ((self.hi as u16) << 8) | (self.lo as u16)
    }

    pub fn set(&mut self, data: u16) {
        self.hi = (data >> 8) as u8;
        self.lo = data as u8;
    }
}