
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
            println!("({}, {}) is out of screen", x, y);
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