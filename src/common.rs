
/// 内存映射
pub(crate) trait Mem {
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


/// 经过一个 CPU 周期
pub(crate) trait Clock {
    type Result; // 有可能需要返回信息
    fn clock(&mut self) -> Self::Result;
}