/// RGB 表示屏幕
pub struct Frame {
    pub data: Vec<u8>,
}

impl Frame {
    pub const WIDTH: usize = 256; // 32 * 8
    pub const HEIGHT: usize = 240; // 30 * 8

    pub fn new() -> Self {
        Frame { data: vec![0; Frame::WIDTH * Frame::HEIGHT * 3] }
    }

    pub fn set_pixel(&mut self, x: usize, y: usize, rgb: (u8, u8, u8)) {
        let base = (y * Frame::WIDTH + x) * 3;
        if base + 2 < self.data.len() {
            self.data[base] = rgb.0;
            self.data[base + 1] = rgb.1;
            self.data[base + 2] = rgb.2;
        } else {
            log::warn!("Attempt to set pixel at ({}, {}) which is out of screen", x, y);
        }
    }
}

/// 左闭右开, 上闭下开矩形
pub struct Rect {
    pub left: usize,
    pub top: usize,
    pub right: usize,
    pub bottom: usize,
}

pub trait Mem {
    fn mem_read(&mut self, addr: u16) -> u8;
    fn mem_write(&mut self, addr: u16, data: u8);

    /// 按照 Little-Endian 读取 2 字节
    fn mem_read_u16(&mut self, addr: u16) -> u16 {
        let lo = self.mem_read(addr) as u16;
        let hi = self.mem_read(addr + 1) as u16;
        (hi << 8) | lo
    }

    /// 按照 Little-Endian 写 2 字节
    fn mem_write_u16(&mut self, addr: u16, data: u16) {
        let hi = (data >> 8) as u8;
        let lo = (data & 0xff) as u8;
        self.mem_write(addr, lo);
        self.mem_write(addr + 1, hi);
    }
}


/// 按照主频对应周期步进
pub trait Clock {
    fn clock(&mut self);
}