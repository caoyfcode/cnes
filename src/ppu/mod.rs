mod registers;

use crate::cartridge::Mirroring;

use self::registers::{controller::ControllerRegister, mask::MaskRegister, status::StatusRegister, scroll::ScrollRegister, addr::AddrRegister};


// PPU memory map
//  _______________ $10000  _______________
// | Mirrors       |       | Mirrors       |
// | $0000-$3FFF   |       | $0000-$3FFF   |
// |_______________| $4000 |_______________|
// | Mirrors       |       |               |
// | $3F00-$3F1F   |       |               |
// |_ _ _ _ _ _ _ _| $3F20 | Palettes      |
// | Sprite Palette|       |               |
// |_ _ _ _ _ _ _ _| $3F10 |               |
// | Image Palette |       |               |
// |_______________| $3F00 |_______________|
// | Mirrors       |       |               |
// | $2000-$2EFF   |       |               |
// |_ _ _ _ _ _ _ _| $3000 |               |
// | Attr Table 3  |       |               |
// |_ _ _ _ _ _ _ _| $2FC0 |               |
// | Name Table 3  |       |               |
// |_ _ _ _ _ _ _ _| $2C00 |               |
// | Attr Table 2  |       | Name Tables   |
// |_ _ _ _ _ _ _ _| $2BC0 | (2KB VRAM)    |
// | Name Table 2  |       |               |
// |_ _ _ _ _ _ _ _| $2800 |               |
// | Attr Table 1  |       |               |
// |_ _ _ _ _ _ _ _| $27C0 |               |
// | Name Table 1  |       |               |
// |_ _ _ _ _ _ _ _| $2400 |               |
// | Attr Table 0  |       |               |
// |_ _ _ _ _ _ _ _| $23C0 |               |
// | Name Table 0  |       |               |
// |_______________| $2000 |_______________|
// | Pattern Table1|       |               |
// |_ _ _ _ _ _ _ _| $1000 | Pattern Tables|
// | Pattern Table0|       | (CHR ROM)     |
// |_______________| $0000 |_______________|

pub struct PPU {
    // registers
    controller: ControllerRegister, // 0x2000 > write
    mask: MaskRegister, // 0x2001 > write
    status: StatusRegister, // 0x2002 < read
    oam_addr: u8, // 0x2003 > write
    scroll: ScrollRegister, // 0x2005 >> write twice
    addr: AddrRegister, // 0x2006 >> write twice
    // 其余组成部分
    chr_rom: Vec<u8>, // cartridge CHR ROM, or Pattern Table
    palette_table: [u8; 32],
    vram: [u8; 2 * 1024], // 2KB VRAM
    oam_data: [u8; 256], // Object Attribute Memory, keep state of sprites
    internal_read_buffer: u8, // 读取 0..=0x3eff (palette 之前), 将得到暂存值
    // 状态信息
    mirroring: Mirroring, // screen miroring
    pub nmi_interrupt: Option<u8>, // 是否生成了 NMI 中断
    scanline: u16, // 扫描行数 0..262, 在 241 时生成 NMI 中断
    cycles: u16, // scanline 内 ppu 周期, 0..341
}

impl PPU {
    pub fn new(chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        PPU {
            controller: ControllerRegister::from_bits_truncate(0),
            mask: MaskRegister::from_bits_truncate(0),
            status: StatusRegister::from_bits_truncate(0),
            oam_addr: 0,
            scroll: ScrollRegister::new(),
            addr: AddrRegister::new(),

            chr_rom,
            palette_table: [0; 32],
            vram: [0; 2 * 1024],
            oam_data: [0; 256],
            internal_read_buffer: 0,

            mirroring,
            nmi_interrupt: None,
            scanline: 0,
            cycles: 0,
        }
    }

    pub fn tick(&mut self, cycles: u8) { // 经过 cycles 个 PPU 周期
        self.cycles += cycles as u16;
        if self.cycles >= 341 {
            self.cycles = 0;
            self.scanline += 1;

            if self.scanline == 241 { // VBLANK
                self.status.insert(StatusRegister::VBLANK_STARTED);
                if self.controller.contains(ControllerRegister::GENERATE_NMI) {
                    self.nmi_interrupt = Some(1);
                }
            }

            if self.scanline >= 262 {
                self.scanline = 0;
                self.nmi_interrupt = None;
                self.status.remove(StatusRegister::VBLANK_STARTED);
            }
        }
    }

    // 将 0x2000..=0x3eff 映射到 vram 下标
    fn vram_mirror_addr(&self, addr: u16) -> u16 {
        let mirrored = addr & 0b0010_1111_1111_1111;
        let vram_index = mirrored - 0x0200;
        match self.mirroring {
            Mirroring::VERTICAL => { // A B A B
                vram_index & 0b0000_0011_1111_1111
            }
            Mirroring::HORIZONTAL => { // A A B B
                if vram_index < 0x0400 {
                    vram_index & 0b0000_0001_1111_1111
                } else {
                    (vram_index & 0b0000_0001_1111_1111) + 0x0200
                }
            }
            _ => vram_index // TODO FOUR_SCREEN
        }
    }

    // registers

    pub fn write_to_controller(&mut self, data: u8) { // 0x2000
        let before_nmi_gen = self.controller.contains(ControllerRegister::GENERATE_NMI);
        self.controller.write(data);
        let after_nmi_gen = self.controller.contains(ControllerRegister::GENERATE_NMI);
        // If the PPU is currently in vertical blank, and the PPUSTATUS ($2002) vblank flag is still set (1), changing the NMI flag in bit 7 of $2000 from 0 to 1 will immediately generate an NMI.
        if !before_nmi_gen && after_nmi_gen && self.status.contains(StatusRegister::VBLANK_STARTED) {
            self.nmi_interrupt = Some(1);
        }
    }

    pub fn write_to_mask(&mut self, data: u8) { // 0x2001
        self.mask.write(data);
    }

    pub fn read_status(&mut self) -> u8 { // 0x2002
        let data = self.status.bits();
        self.status.remove(StatusRegister::VBLANK_STARTED);
        self.addr.reset_latch();
        self.scroll.reset_latch();
        data
    }


    pub fn write_to_oam_addr(&mut self, data: u8) { // 0x2003
        self.oam_addr = data;
    }

    pub fn write_to_oam_data(&mut self, data: u8) { // 0x2004
        self.oam_data[self.oam_addr as usize] = data;
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    pub fn read_oam_data(&self) -> u8 {
        self.oam_data[self.oam_addr as usize]
    }

    pub fn write_to_scroll(&mut self, data: u8) { // 0x2005
        self.scroll.write(data);
    }

    pub fn write_to_addr(&mut self, data: u8) { // 0x2006
        self.addr.write(data);
    }

    pub fn write_to_data(&mut self, data: u8) { // 0x2007
        let addr = self.addr.get();
        match addr {
            0..=0x1fff => { // 0..=0b0001_1111_1111_1111
                panic!("attempt to write to chr rom space {:04x}", addr)
            }
            0x2000..=0x3eff => { // 0b0010_0000_0000_0000..=0b0011_1110_1111_1111
                let addr = self.vram_mirror_addr(addr);
                self.vram[addr as usize] = data;
            }
            0x3f00..=0x3fff => {
                let addr = addr & 0b0011_1111_0001_1111;
                self.palette_table[addr as usize - 0x3f00] = data;
            }
            _ => {
                panic!("unexpected access to mirrored space {:04x}", addr)
            }
        }
        self.addr.increment(self.controller.vram_addr_increment());
    }

    pub fn read_data(&mut self) -> u8 {
        let addr = self.addr.get();
        self.addr.increment(self.controller.vram_addr_increment());
        match addr {
            0..=0x1fff => {
                let result = self.internal_read_buffer;
                self.internal_read_buffer = self.chr_rom[addr as usize];
                result
            }
            0x2000..=0x3eff => {
                let result = self.internal_read_buffer;
                let addr = self.vram_mirror_addr(addr);
                self.internal_read_buffer = self.vram[addr as usize];
                result
            }
            0x3f00..=0x3fff => {
                let addr = addr & 0b0011_1111_0001_1111;
                self.palette_table[addr as usize - 0x3f00]
            }
            _ => {
                panic!("unexpected access to mirrored space {:04x}", addr)
            }
        }
    }

    pub fn write_to_oam_dma(&mut self, data: &[u8; 256]) { // 0x4014
        for x in data.iter() {
            self.oam_data[self.oam_addr as usize] = *x;
            self.oam_addr = self.oam_addr.wrapping_add(1);
        }
    }
}