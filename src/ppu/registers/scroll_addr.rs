
/// 包含 PPUADDR, PPUSCROLL 与 PPUCTRL 的 2bit NN
/// 
/// 这里其实包含了 PPU internal registers, 其行为见 https://www.nesdev.org/wiki/PPU_scrolling
pub(in crate::ppu) struct ScrollAddrRegister {
    /// v 与 t 格式如下
    /// ```txt
    /// yyy NN YYYYY XXXXX
    /// ||| || ||||| +++++-- coarse X scroll
    /// ||| || +++++-------- coarse Y scroll
    /// ||| ++-------------- nametable select
    /// +++----------------- fine Y scroll
    /// ```
    /// v 用于在渲染背景时作地址, 同时也作为 PPUADDR, 并且 2bit NN 即为 PPUCTRL 的 NN, NN 用于渲染时更新 coarse/fine scroll
    /// - 在用作渲染背景地址时, v 中的 NN 与 coarse/fine scroll 只在特定时候从 (t, x) 中更新 
    /// - 作为 PPUADDR 时, v 在完成两次书写时才更新, 因而首先会写道 t 中
    v: u16, // Current vram address(15bits)
    t: u16, // Temporary VRAM address (15 bits), can also be thought of as the address of the top left onscreen tile.
    x: u8, // Fine x scroll(3bits), (t, x) 共同可以表示 scroll_x, scroll_y
    w: bool, // w, First or second write toggle. false: x/hi; true: y/lo
}

impl ScrollAddrRegister {
    const COARSE_X_MASK: u16 = 0b11111;
    const COARSE_Y_MASK: u16 = 0b11111_00000;
    const NT_X_MASK: u16 = 0b1_00000_00000;
    const NT_Y_MASK: u16 = 0b10_00000_00000;
    const FINE_Y_MASK: u16 = 0b111_00_00000_00000;
    const ADDR_MIRROR: u16 = 0x3fff; // ppu 地址空间只有 14bit
    
    pub fn new() -> Self {
        ScrollAddrRegister {
            v: 0,
            t: 0,
            x: 0,
            w: false,
        }
    }

    /// $2000, PPUCTRL lower 2 bits
    pub fn write_nametable_select(&mut self, nn: u8) {
        let nt_mask = Self::NT_Y_MASK | Self::NT_X_MASK;
        self.v &= !nt_mask;
        self.v |= (nn as u16) << 10;
    }

    /// on read $2002, PPUSTATUS, reset PPUADDR/PPUSCROLL latch
    pub fn reset_toggle(&mut self) {
        self.w = false;
    }

    /// $2005, PPUSCROLL, write*2
    pub fn write_scroll(&mut self, data: u8) {
        let fine = data & 0b111; // 细 x/y
        let coarse = data >> 3; // 粗 x/y
        if self.w { // second, y
            self.t &= !(Self::FINE_Y_MASK | Self::COARSE_Y_MASK); // 清空
            self.t |= (fine as u16) << 12;
            self.t |= (coarse as u16) << 5;
        } else { // first, x
            self.x = fine;
            self.t &= !Self::COARSE_X_MASK;
            self.t |= coarse as u16;
        }

        self.w = !self.w;
    }

    /// $2006, PPUADDR, write*2
    pub fn write_addr(&mut self, data: u8) {
        if self.w { // second, low
            self.t &= 0xff00;
            self.t |= data as u16;
            self.v = self.t;
            self.v &= Self::ADDR_MIRROR;
        } else { // first, hi
            self.t &= 0x00ff;
            self.t |= ((data & 0b11_1111) as u16) << 8;
        }

        self.w = !self.w;
    }

    /// 得到 PPUADDR 的值, 内部使用
    pub fn get_addr(&self) -> u16 {
        self.v
    }

    /// At dot 256 of each scanline.
    /// If rendering is enabled, the PPU increments the vertical position in v.
    /// The effective Y scroll coordinate is incremented, which is a complex operation that will correctly skip the attribute table memory regions, and wrap to the next nametable appropriately.
    pub fn increment_y_in_v(&mut self) {
        if (self.v & Self::FINE_Y_MASK) != Self::FINE_Y_MASK {    // if fine Y < 7
            self.v += 0x1000;                                     // increment fine Y
        } else {
            self.v &= !Self::FINE_Y_MASK;                         // fine Y = 0
            let mut y = (self.v & Self::COARSE_Y_MASK) >> 5; // let y = coarse Y
            if y == 29 {
                y = 0;                                            // coarse Y = 0
                self.v ^= Self::NT_Y_MASK;                        // switch vertical nametable
            } else if y == 31 {
                y = 0;                                            // coarse Y = 0, nametable not switched
            } else {
                y += 1;                                           // increment coarse Y
            }
            self.v = (self.v & !Self::COARSE_Y_MASK) | (y << 5);  // put coarse Y back into v
        }   
    }

    /// At dot 257 of each scanline.
    /// If rendering is enabled, the PPU copies all bits related to horizontal position from t to v.
    pub fn copy_x_to_v(&mut self) {
        let x_mask = Self::NT_X_MASK | Self::COARSE_X_MASK;
        self.v &= !x_mask;
        self.v |= self.t & x_mask;
    }

    /// During dots 280 to 304 of the pre-render scanline (end of vblank).
    /// If rendering is enabled, at the end of vblank, shortly after the horizontal bits are copied from t to v at dot 257, the PPU will repeatedly copy the vertical bits from t to v from dots 280 to 304, completing the full initialization of v from t.
    pub fn copy_y_to_v(&mut self) {
        let y_mask = Self::FINE_Y_MASK | Self::NT_Y_MASK | Self::COARSE_Y_MASK;
        self.v &= !y_mask;
        self.v |= self.t & y_mask;
    }

    /// Between dot 328 of a scanline, and 256 of the next scanline.
    /// If rendering is enabled, the PPU increments the horizontal position in v many times across the scanline,
    /// it begins at dots 328 and 336, and will continue through the next scanline at 8, 16, 24... 240, 248, 256 (every 8 dots across the scanline until 256).
    /// Across the scanline the effective coarse X scroll coordinate is incremented repeatedly, which will also wrap to the next nametable appropriately.
    pub fn increment_x_in_v(&mut self) {
        if (self.v & Self::COARSE_X_MASK) == 31 { // if coarse X == 31
            self.v &= !Self::COARSE_X_MASK;       // coarse X = 0
            self.v ^= Self::NT_X_MASK;            // switch horizontal nametable
        } else {
            self.v += 1;                          // increment coarse X
        }
    }


    /// on read/write $2007, PPUDATA, add 1 or 32
    pub fn increment_addr(&mut self, val: u8) {
        self.v = self.v.wrapping_add(val as u16);
        self.v &= Self::ADDR_MIRROR;
    }

    pub fn tile_addr(&self) -> u16 {
        0x2000 | (self.v & 0x0fff)
    }

    pub fn attr_addr(&self) -> u16 {
        // low 12 bits
        // NN 1111 YYY XXX
        // || |||| ||| +++-- high 3 bits of coarse X (x/4)
        // || |||| +++------ high 3 bits of coarse Y (y/4)
        // || ++++---------- attribute offset (960 bytes)
        // ++--------------- nametable select
        0x23c0 | (self.v & 0x0c00) | ((self.v >> 4) & 0x38) | ((self.v >> 2) & 0x07)
    }

    pub fn scroll_x(&self) -> u8 {
        ((self.t & Self::COARSE_X_MASK) << 3) as u8 + self.x
    }

    pub fn scroll_y(&self) -> u8 {
       (((self.t & Self::COARSE_Y_MASK) >> 5) << 3) as u8 +
       ((self.t & Self::FINE_Y_MASK) >> 12) as u8
    }
}