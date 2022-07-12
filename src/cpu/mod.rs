/// # CPU struct
/// `status`: NV-BDIZC(Negative, Overflow, Break, Decimal, Interrupt Disable, Zero, Carry)
pub struct CPU {
    pub register_a: u8,
    pub register_x: u8,
    pub status: u8,
    pub program_counter: u16,
    memory: [u8; 0xFFFF],
}

impl CPU {
    pub fn new() -> Self {
        CPU {
            register_a: 0,
            register_x: 0,
            status: 0,
            program_counter: 0,
            memory: [0; 0xFFFF],
        }
    }

    fn mem_read(&self, addr: u16) -> u8 {
        self.memory[addr as usize]
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        self.memory[addr as usize] = data;
    }

    /// 按照 Little-Endian 读取 2 字节
    fn mem_read_u16(&self, addr: u16) -> u16 {
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

    pub fn load_and_run(&mut self, program: Vec<u8>) {
        self.load(program);
        self.reset();
        self.run();
    }

    /// 模拟 NES 插入卡带时的动作
    /// 1. 状态重置(寄存器与状态寄存器)
    /// 2. 将 PC 寄存器值设为地址 0xFFFC 处的 16 bit 数值
    pub fn reset(&mut self) {
        self.register_a = 0;
        self.register_x = 0;
        self.status = 0;

        self.program_counter = self.mem_read_u16(0xFFFC);
    }

    /// 1. 将 ROM 加载至 0x8000 至 0xFFFF
    /// 2. 设置程序开始地址
    pub fn load(&mut self, program: Vec<u8>) {
        self.memory[0x8000 .. (0x8000 + program.len())].copy_from_slice(&program[..]);
        self.mem_write_u16(0xFFFC, 0x8000);
    }

    pub fn run(&mut self) {
        loop {
            let opcode = self.mem_read(self.program_counter);
            self.program_counter += 1;

            match opcode {
                // mode, syntax, len, time, flags
                0xA9 => { // Immediate, LDA #$44, 2, 2, NZ
                    let param = self.mem_read(self.program_counter);
                    self.program_counter += 1;

                    self.lda(param);
                }
                0xAA => { // Implied, TAX, 1, 2, NZ
                    self.tax();
                }
                0xE8 => { // Implied, INX, 1, 2, NZ
                    self.inx();
                }
                0x00 => { // Implied, BRK, 1, 7
                    return;  // just end
                }
                _ => todo!()
            }
        }
    }

    fn lda(&mut self, value: u8) {
        self.register_a = value;

        self.update_flag_nz(self.register_a);
    }

    fn tax(&mut self) {
        self.register_x = self.register_a;

        self.update_flag_nz(self.register_x);
    }

    fn inx(&mut self) {
        (self.register_x, _ ) =self.register_x.overflowing_add(1);

        self.update_flag_nz(self.register_x);
    }

    fn update_flag_nz(&mut self, result: u8) {
        if result == 0 { // Z
            self.status = self.status | 0b0000_0010;
        } else {
            self.status = self.status & 0b1111_1101;
        }

        if result & 0b1000_0000 != 0 { // N
            self.status = self.status | 0b1000_0000;
        } else {
            self.status = self.status & 0b0111_1111;
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_0xa9_lda_immidiate_load_data() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![0xa9, 0x05, 0x00]); // LDA #$05; BRK
        assert_eq!(cpu.register_a, 0x05);
        assert!(cpu.status & 0b0000_0010 == 0b00); // Z is 0
        assert!(cpu.status & 0b1000_0000 == 0b00); // N is 0
    }

    #[test]
    fn test_0xa9_lda_zero_flag() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![0xa9, 0x00, 0x00]); // LDA #$00; BRK
        assert_eq!(cpu.register_a, 0x00);
        assert!(cpu.status & 0b0000_0010 == 0b10); // Z is 1
    }

    #[test]
    fn test_0xa9_lda_neg() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![0xa9, 0xff, 0x00]); // LDA #$FF; BRK
        assert_eq!(cpu.register_a, 0xff);
        assert!(cpu.status & 0b1000_0000 == 0b1000_0000); // N is 1
    }

    #[test]
    fn test_0xaa_tax_move_a_to_x() {
        let mut cpu = CPU::new();
        cpu.load(vec![0xaa, 0x00]);  // TAX; BRK
        cpu.reset();
        cpu.register_a = 10;
        cpu.run();

        assert_eq!(cpu.register_x, 10)
    }

    #[test]
    fn test_5_ops_working_together() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![0xa9, 0xc0, 0xaa, 0xe8, 0x00]); // LDA #$c0; TAX; INX; BRK

        assert_eq!(cpu.register_x, 0xc1)
    }

    #[test]
    fn test_inx_overflow() {
        let mut cpu = CPU::new();
        cpu.load(vec![0xe8, 0xe8, 0x00]); // INX; INX; BRK
        cpu.reset();
        cpu.register_x = 0xff;
        cpu.run();

        assert_eq!(cpu.register_x, 1)
    }
}
