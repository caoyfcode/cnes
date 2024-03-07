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
#[derive(Clone, Copy)]
struct Sprite {
    y: u8,
    tile_index: u8,
    attributes: u8,
    x: u8,
    tile: [u8; 16],
    other_tile: [u8; 16], // for 8x16 sprite
}

impl Sprite {
    const fn new() -> Self {
        Self {
            y: 0xff,
            tile_index: 0xff,
            attributes: 0xff,
            x: 0xff,
            tile: [0xff; 16],
            other_tile: [0xff; 16],
        }
    }

    fn get_pixel(&self, x: u8, y: u8, h_is_16: bool) -> Option<u8> {
        let h = if h_is_16 { 16u16 } else { 8u16 };
        let w = 8u16;
        if x < self.x || x as u16 >= self.x as u16 + w || y < self.y || y as u16 >= self.y as u16 + h {
            return None;
        }
        let mut xx = x - self.x; 
        let mut yy = (y - self.y) as usize;
        if self.flip_h() {
            xx = w as u8 - 1 - xx;
        }
        if self.flip_v() {
            yy = h as usize - 1 - yy;
        }
        if yy < 8 {
            let lo = (self.tile[yy] >> (7 - xx)) & 1;
            let hi = (self.tile[yy + 8] >> (7 - xx)) & 1;
            Some((hi << 1) | lo)
        } else {
            yy -= 8;
            let lo = (self.other_tile[yy] >> (7 - xx)) & 1;
            let hi = (self.other_tile[yy + 8] >> (7 - xx)) & 1;
            Some((hi << 1) | lo)
        }
    }

    fn flip_v(&self) -> bool {
        self.attributes & 0b1000_0000 == 0b1000_0000
    }

    fn flip_h(&self) -> bool {
        self.attributes & 0b0100_0000 == 0b0100_0000
    }

    fn priority_bit(&self) -> u8 {
        (self.attributes >> 5) & 0x1 
    }

    fn palette_idx(&self) -> u8 {
        self.attributes & 0b11
    }
}

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
    // Sprite rendering states
    current_sprites: [Sprite; 8], // 本行要渲染的 8 个 sprite, 但其实坐标 y 为上一行, 却延后一行渲染
    second_oam: [u8; 32], // 本行寻找下一行渲染的 sprite, 放置在 second OAM 中, 下一行要渲染, 但坐标 y 是本行
    second_oam_n: usize, // 当前 second OAM 中有了几个 sprite
    sprite_eval_n: usize, // 从 OAM 中读到了第几个 sprite
    sprite_eval_m: usize, // m = 0, 1, 2, 3, 表示一个 sprite 的 4 个字节
    sprite_eval_tmp_data: u8,
    sprite_eval_done: bool, // 表示是否 64 个 OAM 都被访问完了
    // 状态信息
    mirroring: Mirroring, // screen miroring
    scanline: u16, // 扫描行数 0..262, 在 241 时生成 NMI 中断
    cycle: u16, // scanline 内 ppu 周期, 0..341
    frame: Frame,
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

            current_sprites: [Sprite::new(); 8],
            second_oam: [0xff; 32],
            second_oam_n: 0,
            sprite_eval_n: 0,
            sprite_eval_m: 0,
            sprite_eval_tmp_data: 0,
            sprite_eval_done: false,
            
            mirroring,
            scanline: 0,
            cycle: 0,
            frame: Frame::new(),
        }
    }

    /// 运行 1 个 PPU 周期
    /// 
    /// ## Background
    /// visible scaline 的 1..=256 与 visible/pre-render scanline的 321..=336,
    /// 每 8 周期进行一次 fetch nt, fetch at, fetch bg lo bits, fetch bg hi bits, 每个两周期.
    /// 并且 shift register 在周期 9, 17, ...(下一个周期的第 1 阶段) 进行 reload. (实现时每个第一阶段都进行)
    /// 并且 PPU internal registers的 v 在 8, 16,..256, 328, 336 更新水平偏移.
    /// 由于 321..=336 这 16 个周期预先获取了 2 个 tile 的背景, 因而每行开始时直接可以渲染,
    /// 从周期 2 开始的话正好周期 9 可以渲染完 1 个 tile, 然后便进行了 reload.
    /// 至于 sprite, 由于上一行的数据已经加载好了, 故可以随着背景的渲染同时渲染.
    /// ## Sprite
    /// 每行 Sprite 处理分为三阶段,
    /// - 1..=64, 用来清空 second OAM
    /// - 65..=256, sprite evaluation, 计算本行有哪些 sprite 可以渲染, 放入 second OAM, 并在下一行进行渲染
    /// - 257..=320, sprite fetch, 根据 second OAM 进行访存, 获取 tile data, 为下一行进行渲染准备
    /// ## 渲染
    /// 在 visible scanline 的 2..=257 周期进行渲染, 每周期一个像素, 共 256 个
    fn tick(&mut self) { 
        let start_of_vblank = matches!((self.scanline, self.cycle), (241, 1));
        let end_of_vblank = matches!((self.scanline, self.cycle), (261, 1));
        let visible_scanline = matches!(self.scanline, 0..=239);
        let rendering_cycle = matches!(
            (self.scanline, self.cycle), 
            (0..=239, 2..=257)
        );
        let rendering_bg_cycle = rendering_cycle &&
            self.mask.contains(MaskRegister::SHOW_BACKGROUND) &&
            (self.mask.contains(MaskRegister::BACKGROUN_LEFTMOST_8PXL) || (self.cycle - 2 > 7));
        let background_fetch_cycle = matches!(
            (self.scanline, self.cycle),
            (0..=239 | 261, 1..=256 | 321..=336)
        );
        let second_oam_init_cycle = matches!(self.cycle, 1..=64);
        let sprite_eval_cycle = matches!(self.cycle, 65..=256);
        let sprite_fetch_cycle = matches!(self.cycle, 257..=320);
        let rendering_spr_cycle = rendering_cycle &&
            self.mask.contains(MaskRegister::SHOW_SPRITES) &&
            (self.mask.contains(MaskRegister::SPRITE_LEFTMOST_8PXL) || (self.cycle - 2 > 7));

        if self.rendering_enabled() {
            if rendering_cycle {
                let (bg_color, bg_zero) = self.background_pixel();
                let spr_pix_ret = self.sprite_pixel();
                let pixel_color = match (bg_zero, spr_pix_ret) {
                    (true, None) => {
                        Self::SYSTEM_PALETTE[self.palettes_ram[0] as usize]
                    }
                    (true, Some((spr_color, _,))) | 
                    (false, Some((spr_color, 0))) => { // 背景为 0 或精灵 priority 为 0, 显示精灵
                        if rendering_spr_cycle {
                            Self::SYSTEM_PALETTE[spr_color]
                        } else {
                            Self::SYSTEM_PALETTE[self.palettes_ram[0] as usize]
                        }
                    }
                    _ => { // 否则显示背景
                        if rendering_bg_cycle {
                            Self::SYSTEM_PALETTE[bg_color]
                        } else {
                            Self::SYSTEM_PALETTE[self.palettes_ram[0] as usize]
                        }
                    }
                };
                self.frame.set_pixel(self.cycle as usize - 2, self.scanline as usize, pixel_color);

                // sprite 0 hit detection
                if self.mask.contains(MaskRegister::SHOW_SPRITES) && self.mask.contains(MaskRegister::SHOW_BACKGROUND) &&
                    ((self.mask.contains(MaskRegister::SPRITE_LEFTMOST_8PXL) && self.mask.contains(MaskRegister::BACKGROUN_LEFTMOST_8PXL)) || 
                    (self.cycle - 2 > 7)) &&
                    self.cycle - 2 != 255 &&
                    !self.status.contains(StatusRegister::SPRITE_ZERO_HIT)
                {
                    let sprite_0 = self.fetch_sprite_0();
                    let spr_0_pix = sprite_0.get_pixel((self.cycle - 2) as u8, self.scanline as u8 - 1, self.controller.contains(ControllerRegister::SPRITE_SIZE));
                    if let Some(pix) = spr_0_pix {
                        if pix != 0 && !bg_zero {
                            self.status.insert(StatusRegister::SPRITE_ZERO_HIT);
                        }
                    }
                }
            }

            if background_fetch_cycle {
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

        if visible_scanline {
            if second_oam_init_cycle {
                // Secondary OAM (32-byte buffer for current sprites on scanline) is initialized to $FF - attempting to read $2004 will return $FF.
                self.second_oam[(self.cycle as usize - 1) / 2] = 0xff;
                self.second_oam_n = 0;
                self.sprite_eval_n = 0;
                self.sprite_eval_m = 0;
                self.sprite_eval_done = false;
            } else if sprite_eval_cycle {
                self.sprite_evaluation();
            } else if sprite_fetch_cycle {
                self.sprite_fetch();
            }
        }

        if start_of_vblank { // start of vblank
            self.status.insert(StatusRegister::VBLANK_STARTED);
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

    // nametable 与 attribute table
    // 每个 nametable 共 1024B, 其中 30*32=960B 用来表示一屏幕所有 tile
    // 而一个 tile 大小为 8*8 像素, 在 pattern table 中用连续的 16B 表示, nametable 前 960B 每个字节表示一个 tile 的索引
    // 每个 namtable 的后 32*32-30*32=2x32B=64B 为 attribute table
    // attribute table 中每 4*4 个 tile 共用一个字节, 而 8*8=64B,  30/4=7.5, 故最后 8B 每个字节只用到了一半
    // 4*4 个 tile 的每 2*2 tile 共用一个调色板, 也就是说每个字节有 4 个调色板(每2bit表示一个)

    /// 每个 scanline 的周期 2...257 每周期得到一个像素(共 256 像素),
    /// 返回值为系统(调色板的索引, is_zero)
    fn background_pixel(&self) -> (usize, bool) {
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
        (color_index % 64, pixel == 0)
    }

    /// 每个 scanline 的周期 2...257 每周期得到一个像素(共 256 像素),
    /// 从本行保存的 sprites 中获取当前的像素, 返回值为(系统调色板的索引, priority)
    fn sprite_pixel(&self) -> Option<(usize, u8)> {
        let y = (self.scanline - 1) as u8; // sprite 的渲染延迟了一行, 故 y = self.scanline - 1
        let x = (self.cycle - 2) as u8;

        for spr in &self.current_sprites {
            let pix = spr.get_pixel(x, y, self.controller.contains(ControllerRegister::SPRITE_SIZE));
            if let Some(pixel) = pix {
                let priority = spr.priority_bit();
                let palette_idx = spr.palette_idx();
                let palette_start = (palette_idx as usize) * 4 + 0x10;
                let color_index = match pixel {
                    0 => continue, // 透明像素不渲染
                    _ => self.palettes_ram[palette_start + pixel as usize] as usize,
                };
                return Some((color_index % 64, priority));
            }
        }
        None
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

    // -- sprite evaluation --
    /// sprite evaluation 阶段是为下一行寻找 8 个 sprite 并放入 second oam 中, 发生在 65..=256 周期,
    /// 奇数周期从 OAM 读一个字节, 偶数周期写到 second OAM 一个字节.
    fn sprite_evaluation(&mut self) {
        if self.cycle % 2 == 1 { // odd cycles, read
            self.sprite_eval_tmp_data = self.oam_data[4 * self.sprite_eval_n + self.sprite_eval_m];
        } else { // even cycles, write
            if !self.sprite_eval_done { // OAM 未访问完全则继续
                if self.second_oam_n < 8 { // 只有在 OAM 与 second OAM 都未访问完全才写
                    self.second_oam[4 * self.second_oam_n + self.sprite_eval_m] = self.sprite_eval_tmp_data;
                }                
                if self.sprite_eval_m == 0 { // 新 sprite 第一个字节
                    let y = self.sprite_eval_tmp_data as u16;
                    let h = if self.controller.contains(ControllerRegister::SPRITE_SIZE) {
                        16u16
                    } else {
                        8u16
                    };
                    if self.scanline >= y && self.scanline < y + h {
                        self.sprite_eval_m = 1;
                        if self.second_oam_n == 8 {
                            self.status.insert(StatusRegister::SPRITE_OVERFLOW);
                        }
                    } else {
                        self.sprite_eval_n += 1;
                        // TODO 未实现 overflow bug
                        // overflow bug 会导致把以后的的第二字节、第三字节、第四字节等当作 Y
                        // if self.second_oam_n == 8 
                        //     self.sprite_eval_m += 1;
                        // }
                    }
                } else {
                    self.sprite_eval_m += 1;
                }
                if self.sprite_eval_m == 4 {
                    self.sprite_eval_m = 0;
                    self.sprite_eval_n += 1;
                    if self.second_oam_n < 8 {
                        self.second_oam_n += 1;
                    }
                }
                if self.sprite_eval_n == 64 {
                    self.sprite_eval_done = true;
                    self.sprite_eval_n = 0;
                }
            }
        }
    }

    /// sprite fetch 阶段, 将 second OAM 中的 sprite 的数据获取并放入 current_sprites 数组, 共 64 个周期, 每个 sprite 占 8 个
    fn sprite_fetch(&mut self) {
        let n = (self.cycle as usize - 257) / 8; // 0..=7 表示第 n 个 sprite
        let cycle = (self.cycle as usize - 257) % 8; // 0..=7 表示 sprite 的第 cycle 个周期
        if n < self.second_oam_n {
            match cycle {
                0 => {
                    self.current_sprites[n].y = self.second_oam[4 * n];
                }
                1 => {
                    self.current_sprites[n].tile_index = self.second_oam[4 * n + 1];
                }
                2 => {
                    self.current_sprites[n].attributes = self.second_oam[4 * n + 2];
                }
                3 => {
                    self.current_sprites[n].x = self.second_oam[4 * n + 3];
                }
                4 => { // 4..=7 这四个周期用来 fetch tile data
                    let tile_index = self.current_sprites[n].tile_index as usize;
                    if !self.controller.contains(ControllerRegister::SPRITE_SIZE) { // 8x8 sprites
                        let bank_base = if self.controller.contains(ControllerRegister::SPRITE_PATTERN_ADDR) {
                            0x1000usize
                        } else {
                            0usize
                        };
                        for idx in 0..16usize {
                            self.current_sprites[n].tile[idx] = self.chr_rom[bank_base + tile_index * 16 + idx];
                        }
                    } else {
                        let bank_base = (tile_index & 0x1) * 0x1000;
                        let tile_index = tile_index >> 1;
                        for idx in 0..16usize {
                            self.current_sprites[n].tile[idx] = self.chr_rom[bank_base + tile_index * 16 + idx];
                        }
                        for idx in 0..16usize {
                            self.current_sprites[n].other_tile[idx] = self.chr_rom[bank_base + tile_index * 16 + 16 + idx];
                        }
                    }
                }
                _ => ()
            }
        } else if n == self.second_oam_n && cycle == 0{  // first empty sprite slot
            self.current_sprites[n].y = self.oam_data[63 * 4]; // TODO 不确定这样实现是否正确
        } else { // other empty slot
            match cycle {
                0 => {
                    self.current_sprites[n].y = 0xff;
                }
                1 => {
                    self.current_sprites[n].tile_index = 0xff;
                }
                2 => {
                    self.current_sprites[n].attributes = 0xff;
                }
                3 => {
                    self.current_sprites[n].x = 0xff;
                }
                _ => ()
            }
        }
        
    }

    fn fetch_sprite_0(&self) -> Sprite {
        let mut sprite_0 = Sprite::new();
        sprite_0.y = self.oam_data[0];
        sprite_0.tile_index = self.oam_data[1];
        sprite_0.attributes = self.oam_data[2];
        sprite_0.x = self.oam_data[3];
        let tile_index = sprite_0.tile_index as usize;
        if !self.controller.contains(ControllerRegister::SPRITE_SIZE) { // 8x8 sprites
            let bank_base = if self.controller.contains(ControllerRegister::SPRITE_PATTERN_ADDR) {
                0x1000usize
            } else {
                0usize
            };
            for idx in 0..16usize {
                sprite_0.tile[idx] = self.chr_rom[bank_base + tile_index * 16 + idx];
            }
        } else {
            let bank_base = (tile_index & 0x1) * 0x1000;
            let tile_index = tile_index >> 1;
            for idx in 0..16usize {
                sprite_0.tile[idx] = self.chr_rom[bank_base + tile_index * 16 + idx];
            }
            for idx in 0..16usize {
                sprite_0.other_tile[idx] = self.chr_rom[bank_base + tile_index * 16 + 16 + idx];
            }
        }
        sprite_0            
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