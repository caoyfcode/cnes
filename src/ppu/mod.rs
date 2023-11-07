mod registers;

use crate::common::Clock;
use registers::{ControllerRegister, MaskRegister, StatusRegister, ScrollAddrRegister};


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
    scroll_addr: ScrollAddrRegister, // 0x2005 >> write twice, 0x2006 >> write twice
    // 其余组成部分
    chr_rom: Vec<u8>, // cartridge CHR ROM, or Pattern Table
    palettes_ram: [u8; 32], // background palette and sprite palette
    vram: [u8; 2 * 1024], // 2KB VRAM
    oam_data: [u8; 256], // Object Attribute Memory, keep state of sprites
    read_buffer: u8, // 读取 PPUDATA 时若地址位于 0..=0x3eff (palette 之前), 将得到暂存值 attributes for the lower 8 pixels of the 16-bit shift register.
    // Background rendering shift registers
    tile_hi_shift_register: u16, // 水平连续两个 tile 的一行16像素的高 bit
    tile_lo_shift_register: u16, // 水平连续两个 tile 的一行16像素的低 bit
    attr_hi_shift_register: u16, // 对应 16 个像素的 attribute 的高 bit (nesdev 中讲解使用了 8bit 的寄存器保存 8 个像素, 为了方便实现, 用了 16 bit)
    attr_lo_shift_register: u16, // 对应 16 个像素的 attribute 的低 bit
    // Background rendering latch
    fetched_tile_addr: usize, // 8 周期中第 1,2 周期读取
    fetched_attribute: u8, // 2bit, 8 周期中第 3,4 周期读取
    fetched_tile_lo: u8, // 8周期中第 5,6 周期读取
    fetched_tile_hi: u8, // 8周期中第 7,8 周期读取
    // 状态信息
    mirroring: Mirroring, // screen miroring
    scanline: u16, // 扫描行数 0..262, 在 241 时生成 NMI 中断
    cycle: u16, // scanline 内 ppu 周期, 0..341
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

impl Ppu {
    pub fn new(chr_rom: Vec<u8>, mirroring: Mirroring) -> Self {
        Ppu {
            controller: ControllerRegister::from_bits_truncate(0),
            mask: MaskRegister::from_bits_truncate(0),
            status: StatusRegister::from_bits_truncate(0),
            oam_addr: 0,
            scroll_addr: ScrollAddrRegister::new(),

            chr_rom,
            palettes_ram: [0; 32],
            vram: [0; 2 * 1024],
            oam_data: [0; 256],
            read_buffer: 0,

            tile_hi_shift_register: 0,
            tile_lo_shift_register: 0,
            attr_hi_shift_register: 0,
            attr_lo_shift_register: 0,
            fetched_tile_addr: 0,
            fetched_attribute: 0,
            fetched_tile_lo: 0,
            fetched_tile_hi: 0,
            
            mirroring,
            scanline: 0,
            cycle: 0,
            frame: Frame::new(),
        }
    }

    /// visible scaline 的 1..=256 与 visible/pre-render scanline的 321..=336,
    /// 每 8 周期进行一次 fetch nt, fetch at, fetch bg lo bits, fetch bg hi bits, 每个两周期.
    /// 并且 shift register 在周期 9, 17, ...(下一个周期的第 1 阶段) 进行 reload. (实现时每个第一阶段都进行)
    /// 并且 PPU internal registers的 v 在 8, 16,..256, 328, 336 更新水平偏移.

    /// 运行 1 个 PPU 周期
    fn tick(&mut self) { 
        let start_of_vblank = matches!((self.scanline, self.cycle), (241, 1));
        let end_of_vblank = matches!((self.scanline, self.cycle), (261, 1));
        let visible_scanline = matches!(self.scanline, 0..=239);
        let rendering_cycle = matches!(
            (self.scanline, self.cycle), 
            (0..=239, 2..=257)
        );
        let fetching_data = matches!(
            (self.scanline, self.cycle),
            (0..=239 | 261, 1..=256 | 321..=336)
        );

        if rendering_cycle && self.rendering_enabled() { // FIXME 这个条件不符合 nesdev 所述
            let bg_color = self.background_pixel();
            self.frame.set_pixel(self.cycle as usize - 2, self.scanline as usize, Self::SYSTEM_PALETTE[bg_color]);
            
            // FIXME is sprite 0 hit, 即是否已经绘制完 sprite 0 的左上角(不正确)
            let sprite_0_y = self.oam_data[0] as u16;
            let sprite_0_x = self.oam_data[3] as u16;
            if sprite_0_y == self.scanline
                && sprite_0_x == (self.cycle - 2)
                && bg_color != 0
                && self.mask.contains(MaskRegister::SHOW_SPRITES) {
                self.status.insert(StatusRegister::SPRITE_ZERO_HIT);
            }
        }

        if self.rendering_enabled() {
            if fetching_data {
                match self.cycle % 8 {
                    1 => {
                        self.reload_shift_registers(); // 第一个周期 shift
                        self.fetch_nametable();
                    },
                    3 => self.fetch_attribute(),
                    5 => self.fetch_tile_lo(),
                    7 => self.fetch_tile_hi(),
                    0 => self.scroll_addr.increment_x_in_v(),
                    _ => (),
                }
            }

            if visible_scanline || self.scanline == 261 {
                match self.cycle {
                    256 => self.scroll_addr.increment_y_in_v(),
                    257 => self.scroll_addr.copy_x_to_v(),
                    _ => (),
                }
            }

            if self.scanline == 261 && self.cycle >= 280 && self.cycle <= 304 {
                self.scroll_addr.copy_y_to_v();
            }
        }

        if start_of_vblank { // start of vblank
            self.status.insert(StatusRegister::VBLANK_STARTED);
            self.update_sprites();
        }

        if end_of_vblank { // end of vlbank
            self.status.remove(StatusRegister::VBLANK_STARTED);
            self.status.remove(StatusRegister::SPRITE_OVERFLOW);
            self.status.remove(StatusRegister::SPRITE_ZERO_HIT);
        }

        if self.cycle >= 341 { // cycle: 0-341
            self.cycle = 0;
            self.scanline = (self.scanline + 1) % 262; // scanleine: 0-161
        } else {
            self.cycle += 1;
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

    fn rendering_enabled(&self) -> bool {
        self.mask.contains(MaskRegister::SHOW_BACKGROUND) || self.mask.contains(MaskRegister::SHOW_SPRITES)
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
    
    fn sprites_palette(&self, palette_idx: usize) -> [u8; 4] {
        let palette_start = palette_idx * 4 + 0x11;
        [
            0,
            self.palettes_ram[palette_start],
            self.palettes_ram[palette_start + 1],
            self.palettes_ram[palette_start + 2]
        ]
    }

    // nametable 与 attribute table
    // 每个 nametable 共 1024B, 其中 30*32=960B 用来表示一屏幕所有 tile
    // 而一个 tile 大小为 8*8 像素, 在 pattern table 中用连续的 16B 表示, nametable 前 960B 每个字节表示一个 tile 的索引
    // 每个 namtable 的后 32*32-30*32=2x32B=64B 为 attribute table
    // attribute table 中每 4*4 个 tile 共用一个字节, 而 8*8=64B,  30/4=7.5, 故最后 8B 每个字节只用到了一半
    // 4*4 个 tile 的每 2*2 tile 共用一个调色板, 也就是说每个字节有 4 个调色板(每2bit表示一个)

    /// 每个 scanline 的周期 2...257 每周期得到一个像素(共 256 像素),
    /// x 为屏幕水平坐标, 返回值为系统调色板的索引
    fn background_pixel(&self) -> usize {
        let cycle_shift = (self.cycle - 2) % 8;
        let lshift = self.scroll_addr.fine_x() as u16 + cycle_shift;
        let rshift = 15 - lshift;
        let pixel_hi = (self.tile_hi_shift_register >> rshift) & 0x1;
        let pixel_lo = (self.tile_lo_shift_register >> rshift) & 0x1;
        let attr_hi = (self.attr_hi_shift_register >> rshift) & 0x1;
        let attr_lo = (self.attr_lo_shift_register >> rshift) & 0x1;
        let pixel = (pixel_hi << 1) + pixel_lo;
        let attr = (attr_hi << 1) + attr_lo;
        let palette_start = (attr as usize) * 4;
        let color_index = match pixel {
            0 => self.palettes_ram[0] as usize,
            _ => self.palettes_ram[palette_start + pixel as usize] as usize,
        };
        color_index % 64
    }

    /// 将一个 tile 的一行与它的 2bit attribute 移入移位寄存器
    fn reload_shift_registers(&mut self) {
        self.tile_hi_shift_register <<= 8;
        self.tile_lo_shift_register <<= 8;
        self.attr_hi_shift_register <<= 8;
        self.attr_lo_shift_register <<= 8;
        self.tile_hi_shift_register |= self.fetched_tile_hi as u16;
        self.tile_lo_shift_register |= self.fetched_tile_lo as u16;
        if self.fetched_attribute & 0b10 == 0b10 {
            self.attr_hi_shift_register |= 0x00ff;
        }
        if self.fetched_attribute & 0b01 == 0b01 {
            self.attr_lo_shift_register |= 0x00ff;
        }
    }

    fn fetch_nametable(&mut self) {
        let addr = self.scroll_addr.tile_addr();
        let index = self.vram_mirror_addr(addr) as usize;
        let namtable_byte = self.vram[index];
        // DCBA98 76543210
        // ---------------
        // 0HNNNN NNNNPyyy
        // |||||| |||||+++- T: Fine Y offset, the row number within a tile
        // |||||| ||||+---- P: Bit plane (0: less significant bit; 1: more significant bit)
        // ||++++-++++----- N: Tile number from name table
        // |+-------------- H: Half of pattern table (0: "left"; 1: "right")
        // +--------------- 0: Pattern table is at $0000-$1FFF
        self.fetched_tile_addr = 
            ((self.controller.contains(ControllerRegister::BACKGROUND_PATTERN_ADDR) as usize) << 12) | // H
            ((namtable_byte as usize) << 4) | // NNNN NNNN
            self.scroll_addr.fine_y() as usize; // yyy
        log::trace!("tile address in vram: {:04x}", self.fetched_tile_addr);
    }

    fn fetch_attribute(&mut self) {
        let addr = self.scroll_addr.attr_addr();
        let index = self.vram_mirror_addr(addr) as usize;
        let attr_byte = self.vram[index];
        // 每 4*4 个 tile 共用一个字节, 其中一字节分为四部分:
        // bit01: 左上角 2*2 个tile, bit23: 右上角 2*2 个 tile
        // bit45: 左下角, bit67: 右下角
        let shift_x = self.scroll_addr.coarse_x() & 0b10;
        let shift_y = self.scroll_addr.coarse_y() & 0b10;
        let shift = shift_x  + (shift_y << 1);
        self.fetched_attribute = (attr_byte >> shift) & 0b11;
    }

    fn fetch_tile_lo(&mut self) {
        self.fetched_tile_lo = self.chr_rom[self.fetched_tile_addr];
    }

    fn fetch_tile_hi(&mut self) {
        self.fetched_tile_hi = self.chr_rom[self.fetched_tile_addr + 8];
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

    /// $2000, PPUCTRL
    pub fn write_to_controller(&mut self, data: u8) {
        self.controller.write(data);
        self.scroll_addr.write_nametable_select(data & 0b11);
        // If the PPU is currently in vertical blank, and the PPUSTATUS ($2002) vblank flag is still set (1), changing the NMI flag in bit 7 of $2000 from 0 to 1 will immediately generate an NMI.
        // 这句话由于 NMI_occurred(vblank started) 为 1, NMI_output 由 0 到 1 (generate_nmi), 显然自动生成 nmi, 故不需要做额外处理
    }

    pub fn write_to_mask(&mut self, data: u8) { // 0x2001
        self.mask.write(data);
    }

    pub fn read_status(&mut self) -> u8 { // 0x2002
        let data = self.status.bits();
        self.status.remove(StatusRegister::VBLANK_STARTED);
        self.scroll_addr.reset_toggle();
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
        self.scroll_addr.write_scroll(data);
    }

    pub fn write_to_addr(&mut self, data: u8) { // 0x2006
        self.scroll_addr.write_addr(data);
    }

    fn increment_vram_addr(&mut self) {
        if self.rendering_enabled() && (self.scanline == 261 || self.scanline <= 239) {
            self.scroll_addr.increment_x_in_v();
            self.scroll_addr.increment_y_in_v();
        } else {
            self.scroll_addr.increment_addr(self.controller.vram_addr_increment());
        }
    }

    pub fn write_to_data(&mut self, data: u8) { // 0x2007
        let addr = self.scroll_addr.get_addr();
        self.increment_vram_addr();
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
    }

    pub fn read_data(&mut self) -> u8 {
        let addr = self.scroll_addr.get_addr();
        self.increment_vram_addr();
        match addr {
            0..=0x1fff => {
                let result = self.read_buffer;
                self.read_buffer = self.chr_rom[addr as usize];
                result
            }
            0x2000..=0x3eff => {
                let result = self.read_buffer;
                let addr = self.vram_mirror_addr(addr);
                self.read_buffer = self.vram[addr as usize];
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