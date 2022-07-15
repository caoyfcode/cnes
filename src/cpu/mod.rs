use std::collections::HashMap;
use bitflags::bitflags;
use crate::opcodes;

/// # 寻址模式
/// 6502 有 15 种寻址模式, 仅仅实现存储器的寻址, 且如果不是对该地址进行一般的读写也不实现
/// ## 非存储器, 非索引的寻址
/// + 隐式寻址(Implied)(**不实现**): 操作数的地址隐含于操作码, 且不是存储器地址
/// + 累加器寻址(Accumulator)(**不实现**): 操作数为 A(the accumulator)
/// + 直接寻址(Immediate): 操作数在指令第二个字节
/// ## 非索引的存储器寻址
/// + 绝对寻址(Absolute): 指令第二三个字节为操作数地址, 小端序
/// + 0 页面寻址(ZeroPage): 指令第二个字节为操作数地址, 只能寻址 0x00..=0xfe (0 页): `LDA $35`
/// + 相对寻址(Relative)(**不实现**): branch 指令使用, 指令的第二个字节为操作数, 加到下一指令的 PC 上
/// + 间接寻址(Indirect)(**不实现**): jmp (三字节指令)使用, 二三字节储存一个地址, 将该地址处的值(16bit)加载到 PC 中, 即该地址处的值是操作数地址: `JMP  ($1000)`
/// + 0 页面间接寻址(**不实现**): jmp 使用, 第二字节是 0 页面的一个地址, 该地址处的值(16bit)为操作数地址
/// ## 基于索引(X, Y)的存储器寻址
/// + 绝对变址寻址(Absolute_X, Absolute_Y): 指令第二三个字节加上 X 或 Y 为操作数地址: `STA $1000,Y`
/// + 0 页面变址寻址(ZeroPage_X, ZeroPage_Y): 指令第二个字节加上 X 或 Y 为操作数地址, 且不进位到 0 页以外 `LDA $C0,X`
/// + Indexed Indirect(Indirect_X): 第二个字节的值(8bit)加上 X(不进位) 是一个地址, 该地址处的值(16bit)是操作数的地址: `LDA ($20,X)`
/// + Indirect Indexed(Indirect_Y): 第二个字节的值是一个地址, 该地址处的值(16bit)加上 Y 是操作数的地址: `LDA ($86),Y`
/// + Indexed Indirect 非 0 页面形式(**不实现**): 指令的二三字节(16bit)加上 X, 后续相同
#[derive(Debug)]
#[allow(non_camel_case_types)]
pub enum AddressingMode {
   Immediate,
   ZeroPage,
   ZeroPage_X,
   ZeroPage_Y,
   Absolute,
   Absolute_X,
   Absolute_Y,
   Indirect_X,
   Indirect_Y,
   NoneAddressing,
}

bitflags! {
    /// CPU 状态寄存器(NV-BDIZC)
    /// - 0 `CARRY`: 进位标志，如果计算结果产生进位，则置 1
    /// - 1 `ZERO`: 零标志，如果结算结果为 0，则置 1
    /// - 2 `INTERRUPT_DISABLE`: 中断去使能标志，置 1 则可屏蔽掉 IRQ 中断
    /// - 3 `DECIMAL`: 十进制模式，未使用
    /// - 4 `BREAK`: BRK，后面解释
    /// - 5 `BREAK2` or `U`: 未使用, 后面解释
    /// - 6 `OVERFLOW`: 溢出标志，如果结算结果产生了溢出，则置 1
    /// - 7 `NEGATIVE`: 负标志，如果计算结果为负，则置 1
    /// 其中, B, U 并非实际位, 在执行某些指令时, 标志位 push 或 pop 时附加这两位
    /// 以区分 BRK 出发还是 IRQ 触发
    /// - PHP 触发, UB=11, push 后对 P 无影响
    /// - BRK 触发, UB=11, push 后 I 置为 1
    /// - IRQ 触发, UB=10, push 后 I 置为 1
    /// - MNI 触发, UB=10, push 后 I 置为 1
    pub struct CpuFlags : u8 {
        const CARRY = 0b0000_0001;
        const ZERO = 0b0000_0010;
        const INTERRUPT_DISABLE = 0b0000_0100;
        const DECIMAL = 0b0000_1000;
        const BREAK = 0b0001_0000;
        const BREAK2 = 0b0010_0000;
        const OVERFLOW = 0b0100_0000;
        const NEGATIVE = 0b1000_0000;
    }
}

/// # CPU struct
/// `status`: NV-BDIZC(Negative, Overflow, Break, Decimal, Interrupt Disable, Zero, Carry)
pub struct CPU {
    pub register_a: u8,
    pub register_x: u8,
    pub register_y: u8,
    pub status: CpuFlags,
    pub program_counter: u16,
    memory: [u8; 0xFFFF],
}

impl CPU {
    pub fn new() -> Self {
        CPU {
            register_a: 0,
            register_x: 0,
            register_y: 0,
            status: CpuFlags::from_bits_truncate(0b100100),
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
        self.register_y = 0;
        self.status = CpuFlags::from_bits_truncate(0b100100);

        self.program_counter = self.mem_read_u16(0xFFFC);
    }

    /// 1. 将 ROM 加载至 0x8000 至 0xFFFF
    /// 2. 设置程序开始地址
    pub fn load(&mut self, program: Vec<u8>) {
        self.memory[0x8000 .. (0x8000 + program.len())].copy_from_slice(&program[..]);
        self.mem_write_u16(0xFFFC, 0x8000);
    }

    pub fn run(&mut self) {
        let ref opcodes: HashMap<u8, &'static opcodes::OpCode> = *opcodes::OPCODES_MAP;

        loop {
            let code = self.mem_read(self.program_counter);
            self.program_counter += 1;
            let program_counter_state = self.program_counter;

            let opcode = opcodes.get(&code).expect(&format!("OpCode {:x} is not recognized", code));

            match code {
                // mode, syntax, len, time, flags
                0xa9 | 0xa5 | 0xb5 | 0xad | 0xbd | 0xb9 | 0xa1 | 0xb1 => {
                    self.lda(&opcode.mode);
                }
                0x85 | 0x95 | 0x8d | 0x9d | 0x99 | 0x81 | 0x91 => {
                    self.sta(&&opcode.mode);
                }
                0xAA => {
                    self.tax();
                }
                0xE8 => {
                    self.inx();
                }
                0x00 => { // BRK
                    return;  // just end
                }
                _ => todo!()
            }

            if program_counter_state == self.program_counter { // 分支跳转可能改变 PC
                self.program_counter += (opcode.len - 1) as u16;
            }
        }
    }

    fn get_operand_address(&self, mode: &AddressingMode) -> u16 {
        match mode {
            AddressingMode::Immediate => self.program_counter,
            AddressingMode::ZeroPage => self.mem_read(self.program_counter) as u16,
            AddressingMode::ZeroPage_X => {
                let pos = self.mem_read(self.program_counter);
                pos.wrapping_add(self.register_x) as u16
            }
            AddressingMode::ZeroPage_Y => {
                let pos = self.mem_read(self.program_counter);
                pos.wrapping_add(self.register_y) as u16
            }
            AddressingMode::Absolute => self.mem_read_u16(self.program_counter),
            AddressingMode::Absolute_X => {
                let pos = self.mem_read_u16(self.program_counter);
                pos.wrapping_add(self.register_x as u16)
            }
            AddressingMode::Absolute_Y => {
                let pos = self.mem_read_u16(self.program_counter);
                pos.wrapping_add(self.register_y as u16)
            }
            AddressingMode::Indirect_X => {
                let base = self.mem_read(self.program_counter);
                let ptr = base.wrapping_add(self.register_x) as u16;
                self.mem_read_u16(ptr)
            }
            AddressingMode::Indirect_Y => {
                let ptr = self.mem_read(self.program_counter);
                let addr_base = self.mem_read_u16(ptr as u16);
                addr_base.wrapping_add(self.register_y as u16)
            }
            AddressingMode::NoneAddressing => {
                panic!("mode {:?} is not supported", mode);
            }
        }
    }

    fn lda(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = value;

        self.update_zero_and_negative_flags(self.register_a);
    }

    fn sta(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_a);
    }

    fn tax(&mut self) {
        self.register_x = self.register_a;

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn inx(&mut self) {
        (self.register_x, _ ) =self.register_x.overflowing_add(1);

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn update_zero_and_negative_flags(&mut self, result: u8) {
        if result == 0 { // Z
            self.status.insert(CpuFlags::ZERO);
        } else {
            self.status.remove(CpuFlags::ZERO);
        }

        if result & 0b1000_0000 != 0 { // N
            self.status.insert(CpuFlags::NEGATIVE);
        } else {
            self.status.remove(CpuFlags::NEGATIVE);
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
        assert!(cpu.status.bits() & 0b0000_0010 == 0b00); // Z is 0
        assert!(cpu.status.bits() & 0b1000_0000 == 0b00); // N is 0
    }

    #[test]
    fn test_0xa9_lda_zero_flag() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![0xa9, 0x00, 0x00]); // LDA #$00; BRK
        assert_eq!(cpu.register_a, 0x00);
        assert!(cpu.status.bits() & 0b0000_0010 == 0b10); // Z is 1
    }

    #[test]
    fn test_0xa9_lda_neg() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![0xa9, 0xff, 0x00]); // LDA #$FF; BRK
        assert_eq!(cpu.register_a, 0xff);
        assert!(cpu.status.bits() & 0b1000_0000 == 0b1000_0000); // N is 1
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
    fn test_lda_from_memory() {
        let mut cpu = CPU::new();
        cpu.mem_write(0x10, 0x55);

        cpu.load_and_run(vec![0xa5, 0x10, 0x00]); // LDA $10; BRK

        assert_eq!(cpu.register_a, 0x55);
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
