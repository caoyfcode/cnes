mod registers;
pub mod frame;
mod palette;

use crate::cartridge::Mirroring;

use self::{registers::{controller::ControllerRegister, mask::MaskRegister, status::StatusRegister, scroll::ScrollRegister, addr::AddrRegister}, frame::Frame};


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
    nmi_interrupt: Option<u8>, // 是否生成了 NMI 中断
    scanline: u16, // 扫描行数 0..262, 在 241 时生成 NMI 中断
    cycles: u16, // scanline 内 ppu 周期, 0..341
    frame: Frame,
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
            frame: Frame::new(),
        }
    }

    pub fn tick(&mut self, cycles: u8) { // 经过 cycles 个 PPU 周期
        self.cycles += cycles as u16;
        if self.cycles >= 341 { // 新的 scanline
            self.cycles = self.cycles - 341;
            self.scanline += 1;

            // is sprite 0 hit, 即是否已经绘制完 sprite 0 的左上角
            let sprite_0_y = self.oam_data[0] as u16;
            let sprite_0_x = self.oam_data[3] as u16;
            if sprite_0_y == self.scanline
                && sprite_0_x <= self.cycles
                && self.mask.contains(MaskRegister::SHOW_SPRITES) {
                self.status.insert(StatusRegister::SPRITE_ZERO_HIT);
            }

            if self.scanline == 241 { // VBLANK
                self.status.insert(StatusRegister::VBLANK_STARTED);
                self.status.remove(StatusRegister::SPRITE_ZERO_HIT); // the sprite zero hit flag should be erased upon entering VBLANK state.
                if self.controller.contains(ControllerRegister::GENERATE_NMI) {
                    self.nmi_interrupt = Some(1);
                }
                self.update_frame();
            }

            if self.scanline >= 262 { // 新的一帧
                self.scanline = 0;
                self.nmi_interrupt = None;
                self.status.remove(StatusRegister::VBLANK_STARTED);
                self.status.remove(StatusRegister::SPRITE_ZERO_HIT);
            }
        }
    }

    /// 检查是否生成了 NMI 中断, 检查将自动重置(take)
    pub fn poll_nmi_interrupt(&mut self) -> Option<u8> {
        self.nmi_interrupt.take()
    }

    /// 是否生成了 NMI 中断信号
    pub fn nmi_interrupt(&self) -> Option<u8> {
        self.nmi_interrupt
    }

    /// 获得此时的屏幕状态
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

    // 将 0x2000..=0x3eff 映射到 vram 下标
    // VERTICAL: A B A B
    // HORIZONTAL: A A B B
    fn vram_mirror_addr(&self, addr: u16) -> u16 {
        let mirrored = addr & 0b0010_1111_1111_1111;
        let vram_index = mirrored - 0x2000;
        let name_table = vram_index / 0x400; // 0, 1, 2, 3
        match (&self.mirroring, name_table) {
            (Mirroring::VERTICAL, 2) | (Mirroring::VERTICAL, 3) => vram_index - 0x800,
            (Mirroring::HORIZONTAL, 1) => vram_index - 0x400,
            (Mirroring::HORIZONTAL, 2) => vram_index - 0x400,
            (Mirroring::HORIZONTAL, 3) => vram_index - 0x800,
            _ => vram_index, // TODO FOUR SCREEN
        }
    }

    // 绘制相关

    /// 更新整个屏幕的像素 (在 scanline 241 之前要完成)
    fn update_frame(&mut self) {
        self.update_background();
        self.update_sprites();
    }

    /// 绘制背景:
    /// 共有 32 * 30 = 960 个 tile, 每个 tile 用 1 字节(name table中)指定 pattern,
    /// 每 4 * 4 个 tile 使用 1 个字节(attribute table中) 指定 background palette
    fn update_background(&mut self) {
        let bank = self.controller.contains(ControllerRegister::BACKGROUND_PATTERN_ADDR) as usize;
        let bank_base = bank * 0x1000;

        for idx in 0..0x03c0usize { // nametable 1
            let tile = self.vram[idx] as usize;
            let tile_x = idx % 32;
            let tile_y = idx / 32;
            let tile_base = bank_base + tile * 16;
            let tile = &self.chr_rom[tile_base..(tile_base + 16)];
            let background_palette = palette::background_palette(self, tile_x, tile_y);

            for y in 0..8usize {
                let lo = tile[y];
                let hi = tile[y + 8];

                for x in 0..8usize {
                    let hi = (hi >> (7 - x)) & 0x1;
                    let lo = (lo >> (7 - x)) & 0x1;
                    let color = ((hi) << 1) | lo;
                    let rgb = palette::SYSTEM_PALETTE[background_palette[color as usize] as usize];
                    self.frame.set_pixel(tile_x * 8 + x, tile_y * 8 + y, rgb);
                }
            }
        }
    }

    /// 绘制 sprites
    /// OAM DATA 共 256 字节, 每个 sprite 用到 4 个字节(共 64 个):
    /// - 0: Y position of top of sprite
    /// - 1: index number
    ///   * for 8 * 8 sprites, this is the tile number of this sprite within the pattern table selected in bit 3 of PPUCTRL
    ///   * For 8 * 16 sprites, the PPU ignores the pattern table selection and selects a pattern table from bit 0 of this number.
    /// - 2: Attributes
    ///   ```txt
    ///   76543210
    ///   ||||||||
    ///   ||||||++- Palette (4 to 7) of sprite
    ///   |||+++--- Unimplemented (read 0)
    ///   ||+------ Priority (0: in front of background; 1: behind background)
    ///   |+------- Flip sprite horizontally
    ///   +-------- Flip sprite vertically
    ///   ```
    /// - 3: X position of left side of sprite.
    fn update_sprites(&mut self) {
        if self.controller.contains(ControllerRegister::SPRITE_SIZE) {
            self.update_sprites_8_16();
        } else {
            self.update_sprites_8_8();
        }
    }

    fn update_sprites_8_8(&mut self) {
        let bank = self.controller.contains(ControllerRegister::SPRITE_PATTERN_ADDR) as usize;
        let bank_base = bank * 0x1000;

        for idx in 0..64usize {
            let sprite_start = idx * 4;
            let tile_y = self.oam_data[sprite_start] as usize;
            let tile = self.oam_data[sprite_start + 1];
            let attributes = self.oam_data[sprite_start + 2];
            let tile_x = self.oam_data[sprite_start + 3] as usize;

            let palette_idx = (attributes & 0b11) as usize;
            let flip_h = attributes & 0b0100_0000 == 0b0100_0000;
            let flip_v = attributes & 0b1000_0000 == 0b1000_0000;
            let _priority = attributes & 0b0010_0000 == 0b0010_0000;

            let tile_base = bank_base + (tile as usize) * 16;
            let tile = &self.chr_rom[tile_base..(tile_base + 16)];
            let sprites_palette = palette::sprites_palette(self, palette_idx);

            for y in 0..8usize {
                let lo = tile[y];
                let hi = tile[y + 8];

                for x in 0..8usize {
                    let hi = (hi >> (7 - x)) & 0x1;
                    let lo = (lo >> (7 - x)) & 0x1;
                    let color = ((hi) << 1) | lo;
                    let rgb = if color == 0 { // 透明, 跳过绘制
                        continue;
                    } else {
                        palette::SYSTEM_PALETTE[sprites_palette[color as usize] as usize]
                    };
                    match (flip_h, flip_v) { // 精灵的绘制精确到像素
                        (false, false) => self.frame.set_pixel(tile_x + x, tile_y + y, rgb),
                        (false, true) => self.frame.set_pixel(tile_x + x, tile_y + 7 - y, rgb),
                        (true, false) => self.frame.set_pixel(tile_x + 7 - x, tile_y + y, rgb),
                        (true, true) => self.frame.set_pixel(tile_x + 7 - x, tile_y + 7 - y, rgb),
                    }

                }
            }
        }
    }

    fn update_sprites_8_16(&mut self) {
        todo!("8 * 16 size sprites not implement")
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
                let addr = addr & 0b0011_1111_0001_1111; // mirroring
                match addr {
                    //  $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
                    0x3f10 | 0x3f14 | 0x3f18 | 0x3f1c => {
                        self.palette_table[addr as usize - 0x3f00 - 0x10] = data;
                    }
                    _ => {
                        self.palette_table[addr as usize - 0x3f00] = data;
                    }
                }
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
                let addr = addr & 0b0011_1111_0001_1111; // mirroring
                match addr {
                    //  $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C
                    0x3f10 | 0x3f14 | 0x3f18 | 0x3f1c => {
                        self.palette_table[addr as usize - 0x3f00 - 0x10]
                    }
                    _ => {
                        self.palette_table[addr as usize - 0x3f00]
                    }
                }
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