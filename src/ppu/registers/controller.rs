use bitflags::bitflags;

bitflags! {
    /// ```txt
    /// 7  bit  0
    /// ---- ----
    /// VPHB SINN
    /// |||| ||||
    /// |||| ||++- Base nametable address
    /// |||| ||    (0 = $2000; 1 = $2400; 2 = $2800; 3 = $2C00)
    /// |||| |+--- VRAM address increment per CPU read/write of PPUDATA
    /// |||| |     (0: add 1, going across; 1: add 32, going down)
    /// |||| +---- Sprite pattern table address for 8x8 sprites
    /// ||||       (0: $0000; 1: $1000; ignored in 8x16 mode)
    /// |||+------ Background pattern table address (0: $0000; 1: $1000)
    /// ||+------- Sprite size (0: 8x8 pixels; 1: 8x16 pixels – see PPU OAM#Byte 1)
    /// |+-------- PPU master/slave select
    /// |          (0: read backdrop from EXT pins; 1: output color on EXT pins)
    /// +--------- Generate an NMI at the start of the
    ///            vertical blanking interval (0: off; 1: on)
    /// ```
    pub(in crate::ppu) struct ControllerRegister: u8 {
        const NAME_TABLE_1 = 0b0000_0001;
        const NAME_TABLE_2 = 0b0000_0010;
        const VRAM_ADDR_INC = 0b0000_0100;
        const SPRITE_PATTERN_ADDR = 0b0000_1000;
        const BACKGROUND_PATTERN_ADDR = 0b0001_0000;
        const SPRITE_SIZE = 0b0010_0000;
        const MASTER_SLAVE = 0b0100_0000;
        const GENERATE_NMI = 0b1000_0000;
    }
}

impl ControllerRegister {
    pub fn write(&mut self, data: u8) {
        self.bits = data;
    }

    // VRAM address increment per CPU read/write of PPUDATA
    pub fn vram_addr_increment(&self) -> u8 {
        if self.contains(Self::VRAM_ADDR_INC) {
            32
        } else {
            1
        }
    }

}