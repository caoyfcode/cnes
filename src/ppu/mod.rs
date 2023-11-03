mod registers;

use crate::common::Clock;
use registers::{ControllerRegister, MaskRegister, StatusRegister, ScrollRegister, AddrRegister};


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

pub(crate) struct Ppu {
    // registers
    controller: ControllerRegister, // 0x2000 > write
    mask: MaskRegister, // 0x2001 > write
    status: StatusRegister, // 0x2002 < read
    oam_addr: u8, // 0x2003 > write
    scroll: ScrollRegister, // 0x2005 >> write twice
    addr: AddrRegister, // 0x2006 >> write twice
    // 其余组成部分
    chr_rom: Vec<u8>, // cartridge CHR ROM, or Pattern Table
    palettes_ram: [u8; 32], // background palette and sprite palette
    vram: [u8; 2 * 1024], // 2KB VRAM
    oam_data: [u8; 256], // Object Attribute Memory, keep state of sprites
    internal_read_buffer: u8, // 读取 0..=0x3eff (palette 之前), 将得到暂存值
    // 状态信息
    mirroring: Mirroring, // screen miroring
    scanline: u16, // 扫描行数 0..262, 在 241 时生成 NMI 中断
    cycles: u16, // scanline 内 ppu 周期, 0..341
    frame: Frame,
}

/// PPU Mirroring type
/// - Horizontal
/// - Vertical
/// - 4 Screen
#[derive(Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub enum Mirroring {
    VERTICAL,
    HORIZONTAL,
    FOUR_SCREEN,
}

/// RGB pixels matrix
pub struct Frame {
    data: Vec<u8>,
}

impl Frame {
    pub const WIDTH: usize = 256; // 32 * 8
    pub const HEIGHT: usize = 240; // 30 * 8

    fn new() -> Self {
        Frame { data: vec![0; Frame::WIDTH * Frame::HEIGHT * 3] }
    }

    fn set_pixel(&mut self, x: usize, y: usize, rgb: (u8, u8, u8)) {
        if x >= Frame::WIDTH || y >= Frame::HEIGHT {
            log::warn!("Attempt to set pixel at ({}, {}) which is out of frame buffer", x, y);
            return;
        }
        let base = (y * Frame::WIDTH + x) * 3;
        self.data[base] = rgb.0;
        self.data[base + 1] = rgb.1;
        self.data[base + 2] = rgb.2;
    }

    pub fn data(&self) -> &[u8] {
        &self.data
    }
}

/// 左闭右开, 上闭下开矩形
struct Rect {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

impl Ppu {
    pub fn new(chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Ppu {
            controller: ControllerRegister::from_bits_truncate(0),
            mask: MaskRegister::from_bits_truncate(0),
            status: StatusRegister::from_bits_truncate(0),
            oam_addr: 0,
            scroll: ScrollRegister::new(),
            addr: AddrRegister::new(),

            chr_rom,
            palettes_ram: [0; 32],
            vram: [0; 2 * 1024],
            oam_data: [0; 256],
            internal_read_buffer: 0,

            mirroring,
            scanline: 0,
            cycles: 0,
            frame: Frame::new(),
        }
    }

    /// 运行 1 个 PPU 周期
    fn tick(&mut self) { 
        // is sprite 0 hit, 即是否已经绘制完 sprite 0 的左上角
        let sprite_0_y = self.oam_data[0] as u16;
        let sprite_0_x = self.oam_data[3] as u16;
        if sprite_0_y == self.scanline
            && sprite_0_x <= self.cycles
            && self.mask.contains(MaskRegister::SHOW_SPRITES) {
            self.status.insert(StatusRegister::SPRITE_ZERO_HIT);
        }

        if self.scanline == 241 && self.cycles == 1 { // start of vblank
            self.status.insert(StatusRegister::VBLANK_STARTED);
            self.update_frame();
        }

        if self.scanline == 261 && self.cycles == 1 { // end of vlbank
            self.status.remove(StatusRegister::VBLANK_STARTED);
            self.status.remove(StatusRegister::SPRITE_OVERFLOW);
            self.status.remove(StatusRegister::SPRITE_ZERO_HIT);
        }

        if self.cycles >= 341 { // cycle: 0-341
            self.cycles = 0;
            self.scanline = (self.scanline + 1) % 262; // scanleine: 0-161
        } else {
            self.cycles += 1;
        }
    }

    pub fn vblank_started(&self) -> bool {
        self.status.contains(StatusRegister::VBLANK_STARTED)
    }

    /// 返回 nmi 线电平
    pub fn nmi_line_level(&self) -> bool {
        // NMI_occurred 推测即为 PPUSTATUS:VBLANK_STARTED
        // NMI_output 推测即为 PPUCTRL:GENERATE_NMI
        if self.status.contains(StatusRegister::VBLANK_STARTED) 
            && self.controller.contains(ControllerRegister::GENERATE_NMI) 
            {
            false
        } else {
            true
        }
    }

    /// 获得此时的屏幕状态
    pub fn frame(&self) -> &Frame {
        &self.frame
    }

}

// render
impl Ppu {

    // Pallete PPU Memory Map
    // The palette for the background runs from VRAM $3F00 to $3F0F;
    // the palette for the sprites runs from $3F10 to $3F1F. Each color takes up one byte.
    // 0x3f00:        Universal background color
    // 0x3f01-0x3f03: Background palette 0
    // 0x3f05-0x3f07: Background palette 1
    // 0x3f09-0x3f0b: Background palette 2
    // 0x3f0d-0x3f0f: Background palette 3
    // 0x3f11-0x3f13: Sprite palette 0
    // 0x3f15-0x3f17: Sprite palette 1
    // 0x3f19-0x3f1b: Sprite palette 2
    // 0x3f1d-0x3f1f: Sprite palette 3
    // Addresses $3F04/$3F08/$3F0C can contain unique data
    // Addresses $3F10/$3F14/$3F18/$3F1C are mirrors of $3F00/$3F04/$3F08/$3F0C. This goes for writing as well as reading.

    /// RGB 表示的系统调色板
    const SYSTEM_PALETTE: [(u8,u8,u8); 64] = [
        (0x80, 0x80, 0x80), (0x00, 0x3D, 0xA6), (0x00, 0x12, 0xB0), (0x44, 0x00, 0x96), (0xA1, 0x00, 0x5E),
        (0xC7, 0x00, 0x28), (0xBA, 0x06, 0x00), (0x8C, 0x17, 0x00), (0x5C, 0x2F, 0x00), (0x10, 0x45, 0x00),
        (0x05, 0x4A, 0x00), (0x00, 0x47, 0x2E), (0x00, 0x41, 0x66), (0x00, 0x00, 0x00), (0x05, 0x05, 0x05),
        (0x05, 0x05, 0x05), (0xC7, 0xC7, 0xC7), (0x00, 0x77, 0xFF), (0x21, 0x55, 0xFF), (0x82, 0x37, 0xFA),
        (0xEB, 0x2F, 0xB5), (0xFF, 0x29, 0x50), (0xFF, 0x22, 0x00), (0xD6, 0x32, 0x00), (0xC4, 0x62, 0x00),
        (0x35, 0x80, 0x00), (0x05, 0x8F, 0x00), (0x00, 0x8A, 0x55), (0x00, 0x99, 0xCC), (0x21, 0x21, 0x21),
        (0x09, 0x09, 0x09), (0x09, 0x09, 0x09), (0xFF, 0xFF, 0xFF), (0x0F, 0xD7, 0xFF), (0x69, 0xA2, 0xFF),
        (0xD4, 0x80, 0xFF), (0xFF, 0x45, 0xF3), (0xFF, 0x61, 0x8B), (0xFF, 0x88, 0x33), (0xFF, 0x9C, 0x12),
        (0xFA, 0xBC, 0x20), (0x9F, 0xE3, 0x0E), (0x2B, 0xF0, 0x35), (0x0C, 0xF0, 0xA4), (0x05, 0xFB, 0xFF),
        (0x5E, 0x5E, 0x5E), (0x0D, 0x0D, 0x0D), (0x0D, 0x0D, 0x0D), (0xFF, 0xFF, 0xFF), (0xA6, 0xFC, 0xFF),
        (0xB3, 0xEC, 0xFF), (0xDA, 0xAB, 0xEB), (0xFF, 0xA8, 0xF9), (0xFF, 0xAB, 0xB3), (0xFF, 0xD2, 0xB0),
        (0xFF, 0xEF, 0xA6), (0xFF, 0xF7, 0x9C), (0xD7, 0xE8, 0x95), (0xA6, 0xED, 0xAF), (0xA2, 0xF2, 0xDA),
        (0x99, 0xFF, 0xFC), (0xDD, 0xDD, 0xDD), (0x11, 0x11, 0x11), (0x11, 0x11, 0x11)
    ];
    
    fn background_palette(&self, nametable_base: usize, tile_x: usize, tile_y: usize) -> [u8; 4] {
        let attr_table_idx = tile_y / 4 * 8 + tile_x / 4;
        let attr_byte = self.vram[nametable_base + 960 + attr_table_idx];
    
        let palette_idx = match (tile_x % 4 / 2, tile_y % 4 / 2) {
            (0,0) => attr_byte & 0b11,
            (1,0) => (attr_byte >> 2) & 0b11,
            (0,1) => (attr_byte >> 4) & 0b11,
            (1,1) => (attr_byte >> 6) & 0b11,
            (_,_) => panic!("should not happen"),
        } as usize;
    
        let palette_start = palette_idx * 4 + 1;
        [
            self.palettes_ram[0],
            self.palettes_ram[palette_start],
            self.palettes_ram[palette_start + 1],
            self.palettes_ram[palette_start + 2]
        ]
    }
    
    fn sprites_palette(&self, palette_idx: usize) -> [u8; 4] {
        let palette_start = palette_idx * 4 + 0x11;
        [
            0,
            self.palettes_ram[palette_start],
            self.palettes_ram[palette_start + 1],
            self.palettes_ram[palette_start + 2]
        ]
    }

    /// 更新整个屏幕的像素 (在 scanline 241 之前要完成)
    fn update_frame(&mut self) {
        self.update_background();
        self.update_sprites();
    }

    /// 绘制背景:
    /// 共有 32 * 30 = 960 个 tile, 每个 tile 用 1 字节(name table中)指定 pattern,
    /// 每 4 * 4 个 tile 使用 1 个字节(attribute table中) 指定 background palette
    fn update_background(&mut self) {
        let scroll_x = self.scroll.scroll_x as usize;
        let scroll_y = if self.scroll.scroll_y >= Frame::HEIGHT as u8 {
            //  "Normal" vertical offsets range from 0 to 239, while values of 240 to 255 are treated as -16 through -1 in a way, but tile data is incorrectly fetched from the attribute table.
            let scroll_y = self.scroll.scroll_y as i8; // 转为负数
            let scroll_y = Frame::HEIGHT as isize + scroll_y as isize; // 取模
            scroll_y as usize
        } else {
            self.scroll.scroll_y as usize
        };

        let (base_nametable, other_nametable): (usize, usize) = match (&self.mirroring, self.controller.base_nametable_address()) {
            (_, 0x2000) | (Mirroring::VERTICAL, 0x2800) | (Mirroring::HORIZONTAL, 0x2400) => (0, 0x0400),
            (_, 0x2c00) | (Mirroring::VERTICAL, 0x2400) | (Mirroring::HORIZONTAL, 0x2800) => (0x0400, 0),
            (_, _) => {
                panic!("Not supported mirroring type {:?}", &self.mirroring);
            }
        };
        let (right_nametable, down_namatable) = match &self.mirroring {
            Mirroring::HORIZONTAL => (base_nametable, other_nametable),
            Mirroring::VERTICAL => (other_nametable, base_nametable),
            _ => panic!("can't be here")
        };

        // 绘制四部分到 Frame
        self.update_nametable_to_frame(
            base_nametable,
            Rect { left: scroll_x, top: scroll_y, right: Frame::WIDTH, bottom: Frame::HEIGHT },
            0, 0
        );
        if scroll_x > 0 {
            self.update_nametable_to_frame(
                right_nametable,
                Rect { left: 0, top: scroll_y, right: scroll_x, bottom: Frame::HEIGHT },
                Frame::WIDTH - scroll_x, 0
            );
        }
        if scroll_y > 0 {
            self.update_nametable_to_frame(
                down_namatable,
                Rect { left: scroll_x, top: 0, right: Frame::WIDTH, bottom: scroll_y },
                0, Frame::HEIGHT - scroll_y
            );
        }
        if scroll_x >0 && scroll_y > 0 {
            self.update_nametable_to_frame(
                other_nametable,
                Rect { left: 0, top: 0, right: scroll_x, bottom: scroll_y },
                Frame::WIDTH - scroll_x, Frame::HEIGHT - scroll_y
            );
        }

    }

    // 将 nametable 的 src 部分(以pixel为单位) 绘制到 frame 的 (dest_left, dest_top) 位置, 并且左闭右开，上闭下开
    fn update_nametable_to_frame(&mut self, nametable_base: usize, src: Rect, dest_left: usize, dest_top: usize) {
        let shift_x = dest_left as isize - src.left as isize;
        let shift_y = dest_top as isize - src.top as isize;
        let bank = self.controller.contains(ControllerRegister::BACKGROUND_PATTERN_ADDR) as usize;
        let bank_base = bank * 0x1000;

        for idx in 0..0x03c0usize { // nametable
            let tile = self.vram[nametable_base + idx] as usize;
            let tile_x = idx % 32;
            let tile_y = idx / 32;
            let tile_base = bank_base + tile * 16;
            let tile = &self.chr_rom[tile_base..(tile_base + 16)];
            let background_palette = self.background_palette(nametable_base, tile_x, tile_y);

            for y in 0..8usize {
                let pixel_y = tile_y * 8 + y;
                if pixel_y < src.top || pixel_y >= src.bottom {
                    continue;
                }
                let lo = tile[y];
                let hi = tile[y + 8];

                for x in 0..8usize {
                    let pixel_x = tile_x * 8 + x;
                    if pixel_x < src.left || pixel_x >= src.right {
                        continue;
                    }
                    let hi = (hi >> (7 - x)) & 0x1;
                    let lo = (lo >> (7 - x)) & 0x1;
                    let color = ((hi) << 1) | lo;
                    let rgb = Self::SYSTEM_PALETTE[background_palette[color as usize] as usize];
                    self.frame.set_pixel( (shift_x + pixel_x as isize) as usize,
                        (shift_y + pixel_y as isize) as usize,
                        rgb);
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
            let sprites_palette = self.sprites_palette(palette_idx);

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
                        Self::SYSTEM_PALETTE[sprites_palette[color as usize] as usize]
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
    
}

// registers
impl Ppu {
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

    pub fn write_to_controller(&mut self, data: u8) { // 0x2000
        self.controller.write(data);
        // If the PPU is currently in vertical blank, and the PPUSTATUS ($2002) vblank flag is still set (1), changing the NMI flag in bit 7 of $2000 from 0 to 1 will immediately generate an NMI.
        // 这句话由于 NMI_occurred(vblank started) 为 1, NMI_output 由 0 到 1 (generate_nmi), 显然自动生成 nmi, 故不需要做额外处理
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
                log::warn!("Attempt to write to chr rom space PPU address {:04x}", addr);
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
                        self.palettes_ram[addr as usize - 0x3f00 - 0x10] = data;
                    }
                    _ => {
                        self.palettes_ram[addr as usize - 0x3f00] = data;
                    }
                }
            }
            _ => {
                log::warn!("Attempt to write to mirrored space PPU address {:04x}", addr);
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
                        self.palettes_ram[addr as usize - 0x3f00 - 0x10]
                    }
                    _ => {
                        self.palettes_ram[addr as usize - 0x3f00]
                    }
                }
            }
            _ => {
                log::warn!("Attempt to read from mirrored space PPU address {:04x}", addr);
                0
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

impl Clock for Ppu {
    type Result = ();
    fn clock(&mut self) {
        self.tick();
        self.tick();
        self.tick();
    }
}