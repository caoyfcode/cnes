use std::collections::HashMap;
use bitflags::bitflags;
use crate::opcodes;

/// # 寻址模式
/// 6502 有 <del>15</del> 13 种寻址模式, 不实现的寻址模式在相应的指令实现处实现
/// ## 非存储器, 非索引的寻址
/// + 隐式寻址(Implied)(**不实现**): 操作数的地址隐含于操作码, 且不是存储器地址
/// + 累加器寻址(Accumulator)(**不实现**): 操作数为 A(the accumulator)
/// + 直接寻址(Immediate): 操作数在指令第二个字节
/// ## 非索引的存储器寻址
/// + 绝对寻址(Absolute): 指令第二三个字节为操作数地址, 小端序
/// + 0 页面寻址(ZeroPage): 指令第二个字节为操作数地址, 只能寻址 0x00..=0xfe (0 页): `LDA $35`
/// + 相对寻址(Relative)(**不实现**): branch 指令使用, 指令的第二个字节为操作数, 加到下一指令的 PC 上
/// + 间接寻址(Indirect)(**不实现**): jmp (三字节指令)使用, 二三字节储存一个地址, 将该地址处的值(16bit)加载到 PC 中, 即该地址处的值是操作数地址: `JMP  ($1000)`
/// + <del>0 页面间接寻址(**不实现**): jmp 使用, 第二字节是 0 页面的一个地址, 该地址处的值(16bit)为操作数地址</del>
/// ## 基于索引(X, Y)的存储器寻址
/// + 绝对变址寻址(Absolute_X, Absolute_Y): 指令第二三个字节加上 X 或 Y 为操作数地址: `STA $1000,Y`
/// + 0 页面变址寻址(ZeroPage_X, ZeroPage_Y): 指令第二个字节加上 X 或 Y 为操作数地址, 且不进位到 0 页以外 `LDA $C0,X`
/// + Indexed Indirect(Indirect_X): 第二个字节的值(8bit)加上 X(不进位) 是一个地址, 该地址处的值(16bit)是操作数的地址: `LDA ($20,X)`
/// + Indirect Indexed(Indirect_Y): 第二个字节的值是一个地址, 该地址处的值(16bit)加上 Y 是操作数的地址: `LDA ($86),Y`
/// + <del>Indexed Indirect 非 0 页面形式(**不实现**): 指令的二三字节(16bit)加上 X, 后续相同</del>
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
    /// - 0 `CARRY`: 进位标志，如果计算结果产生进位，则置 1(同时 !CARRY 作为减法的借位标志)
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
    pub stack_pointer: u8,  // 指向空位置
    memory: [u8; 0xFFFF],
}

const STACK: u16 = 0x0100; // stack pointer + STACK 即为真正的栈指针
const STACK_RESET: u8 = 0xfd;

impl CPU {
    pub fn new() -> Self {
        CPU {
            register_a: 0,
            register_x: 0,
            register_y: 0,
            status: CpuFlags::from_bits_truncate(0b100100),
            program_counter: 0,
            stack_pointer: STACK_RESET,
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
        self.stack_pointer = STACK_RESET;

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
                // load/store
                0xa9 | 0xa5 | 0xb5 | 0xad | 0xbd | 0xb9 | 0xa1 | 0xb1 => {
                    self.lda(&opcode.mode);
                }
                0xa2 | 0xa6 | 0xb6 | 0xae | 0xbe => {
                    self.ldx(&opcode.mode);
                }
                0xa0 | 0xa4 | 0xb4 | 0xac | 0xbc => {
                    self.ldy(&opcode.mode);
                }
                0x85 | 0x95 | 0x8d | 0x9d | 0x99 | 0x81 | 0x91 => {
                    self.sta(&&opcode.mode);
                }
                0x86 | 0x96 | 0x8e => {
                    self.stx(&opcode.mode);
                }
                0x84 | 0x94 | 0x8c => {
                    self.sty(&opcode.mode);
                }
                // push/pop
                0x48 => {
                    self.pha();
                }
                0x08 => {
                    self.php();
                }
                0x68 => {
                    self.pla();
                }
                0x28 => {
                    self.plp();
                }
                // 递增/递减
                0xc6 | 0xd6 | 0xce | 0xde => {
                    self.dec(&opcode.mode);
                }
                0xca => {
                    self.dex();
                }
                0x88 => {
                    self.dey();
                }
                0xe6 | 0xf6 | 0xee | 0xfe => {
                    self.inc(&opcode.mode);
                }
                0xe8 => {
                    self.inx();
                }
                0xc8 => {
                    self.iny();
                }
                // 移位
                0x0a => {
                    self.asl_a();
                }
                0x06 | 0x16 | 0x0e | 0x1e => {
                    self.asl(&opcode.mode);
                }
                0x4a => {
                    self.lsr_a();
                }
                0x46 | 0x56 | 0x4e | 0x5e => {
                    self.lsr(&opcode.mode);
                }
                0x2a => {
                    self.rol_a();
                }
                0x26 | 0x36 | 0x2e | 0x3e => {
                    self.rol(&opcode.mode);
                }
                0x6a => {
                    self.ror_a();
                }
                0x66 | 0x76 | 0x6e | 0x7e => {
                    self.ror(&opcode.mode);
                }
                // 逻辑
                0x29 | 0x25 | 0x35 | 0x2d | 0x3d | 0x39 | 0x21 | 0x31 => {
                    self.and(&opcode.mode);
                }
                0x09 | 0x05 | 0x15 | 0x0d | 0x1d | 0x19 | 0x01 | 0x11 => {
                    self.ora(&opcode.mode);
                }
                0x49 | 0x45 | 0x55 | 0x4d | 0x5d | 0x59 | 0x41 | 0x51 => {
                    self.eor(&opcode.mode);
                }
                // bit
                0x24 | 0x2c => {
                    self.bit(&opcode.mode);
                }
                // 比较
                0xc9 | 0xc5 | 0xd5 | 0xcd | 0xdd | 0xd9 | 0xc1 | 0xd1 => {
                    self.cmp(&opcode.mode);
                }
                0xe0 | 0xe4 | 0xec => {
                    self.cpx(&opcode.mode);
                }
                0xc0 | 0xc4 | 0xcc => {
                    self.cpy(&opcode.mode);
                }
                // 算术
                0x69 | 0x65 | 0x75 | 0x6d | 0x7d | 0x79 | 0x61 | 0x71 => {
                    self.adc(&opcode.mode);
                }
                0xe9 | 0xe5 | 0xf5 | 0xed | 0xfd | 0xf9 | 0xe1 | 0xf1 => {
                    self.sbc(&opcode.mode);
                }
                0xaa => {
                    self.tax();
                }
                0xa8 => {
                    self.tay();
                }
                0xba => {
                    self.tsx();
                }
                0x8a => {
                    self.txa();
                }
                0x9a => {
                    self.txs();
                }
                0x98 => {
                    self.tya();
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

    fn ldx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_x = value;

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn ldy(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_y = value;

        self.update_zero_and_negative_flags(self.register_y);
    }

    fn sta(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_a);
    }

    fn stx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_x);
    }

    fn sty(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.mem_write(addr, self.register_y);
    }

    fn pha(&mut self) {
        self.stack_push(self.register_a);
    }

    fn php(&mut self) {
        let mut status = self.status.clone();
        status.insert(CpuFlags::BREAK2);
        status.insert(CpuFlags::BREAK);  // UB = 11 if PHP
        self.stack_push(status.bits());
    }

    fn pla(&mut self) {
        self.register_a = self.stack_pop();
        self.update_zero_and_negative_flags(self.register_a);
    }

    fn plp(&mut self) {
        self.status.bits = self.stack_pop();
        self.status.insert(CpuFlags::BREAK2);
        self.status.remove(CpuFlags::BREAK);
    }

    fn stack_push(&mut self, data: u8) {
        self.mem_write(STACK + self.stack_pointer as u16, data);
        self.stack_pointer = self.stack_pointer.wrapping_sub(1);
    }

    fn stack_pop(&mut self) -> u8 {
        self.stack_pointer = self.stack_pointer.wrapping_add(1);
        self.mem_read(STACK + self.stack_pointer as u16)
    }

    fn dec(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr).wrapping_sub(1);
        self.mem_write(addr, value);

        self.update_zero_and_negative_flags(value);
    }

    fn dex(&mut self) {
        self.register_x = self.register_x.wrapping_sub(1);

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn dey(&mut self) {
        self.register_y = self.register_y.wrapping_sub(1);

        self.update_zero_and_negative_flags(self.register_y);
    }

    fn inc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr).wrapping_add(1);
        self.mem_write(addr, value);

        self.update_zero_and_negative_flags(value);
    }

    fn inx(&mut self) {
        self.register_x = self.register_x.wrapping_add(1);

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn iny(&mut self) {
        self.register_y = self.register_y.wrapping_add(1);

        self.update_zero_and_negative_flags(self.register_y);
    }

    fn asl_a(&mut self) {
        self.register_a = self.arithmetic_shift_left_update_nzc(self.register_a);
    }

    fn asl(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.arithmetic_shift_left_update_nzc(data);
        self.mem_write(addr, data);
    }

    fn arithmetic_shift_left_update_nzc(&mut self, data: u8) -> u8 {
        if data & 0x80 == 0x80 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        let result = data << 1;
        self.update_zero_and_negative_flags(result);
        result
    }

    fn lsr_a(&mut self) {
        self.register_a = self.logical_shift_right_update_nzc(self.register_a);
    }

    fn lsr(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.logical_shift_right_update_nzc(data);
        self.mem_write(addr, data);
    }

    fn logical_shift_right_update_nzc(&mut self, data:u8) -> u8 {
        if data & 0x1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        let result = data >> 1;
        self.update_zero_and_negative_flags(result);
        result
    }

    fn rol_a(&mut self) {
        self.register_a = self.rotate_left_through_carry_update_nzc(self.register_a);
    }

    fn rol(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.rotate_left_through_carry_update_nzc(data);
        self.mem_write(addr, data);
    }

    fn rotate_left_through_carry_update_nzc(&mut self, data: u8) -> u8 {
        let result = (data << 1) +
            if self.status.contains(CpuFlags::CARRY) {
                1u8
            } else {
                0u8
            };
        if data & 0x80 == 0x80 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        self.update_zero_and_negative_flags(result);
        result
    }

    fn ror_a(&mut self) {
        self.register_a = self.rotate_right_through_carry_update_nzc(self.register_a);
    }

    fn ror(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.rotate_right_through_carry_update_nzc(data);
        self.mem_write(addr, data);
    }

    fn rotate_right_through_carry_update_nzc(&mut self, data: u8) -> u8 {
        let result = (data >> 1) +
            if self.status.contains(CpuFlags::CARRY) {
                0x80u8
            } else {
                0u8
            };
        if data & 0x1 == 1 {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        self.update_zero_and_negative_flags(result);
        result
    }

    fn and(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = self.register_a & value;

        self.update_zero_and_negative_flags(self.register_a);
    }

    fn ora(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = self.register_a | value;

        self.update_zero_and_negative_flags(self.register_a);
    }

    fn eor(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.register_a = self.register_a ^ value;

        self.update_zero_and_negative_flags(self.register_a);
    }

    fn bit(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        self.update_zero_and_negative_flags(self.register_a & value); // 仅仅用来设置 Z
        if value & 0x80 == 0x80 { // N
            self.status.insert(CpuFlags::NEGATIVE);
        } else {
            self.status.remove(CpuFlags::NEGATIVE);
        }
        if value & 0x40 == 0x40 { // V
            self.status.insert(CpuFlags::OVERFLOW);
        } else {
            self.status.remove(CpuFlags::OVERFLOW);
        }
    }

    fn cmp(&mut self, mode: &AddressingMode) {
        self.compare_update_nzc(self.register_a, mode);
    }

    fn cpx(&mut self, mode: &AddressingMode) {
        self.compare_update_nzc(self.register_x, mode);
    }

    fn cpy(&mut self, mode: &AddressingMode) {
        self.compare_update_nzc(self.register_y, mode);
    }

    fn compare_update_nzc(&mut self, lhs: u8, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);
        let result = lhs as u16 + (!value) as u16 + 1;

        // CARRY
        if result > 0xff {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
        self.update_zero_and_negative_flags(result as u8);
    }

    fn adc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);

        self.add_to_a_with_carry_update_nvzc(value);
    }

    fn sbc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let value = self.mem_read(addr);

        // A 寄存器 A, M 操作数, B borrow bit, C carry bit
        // A <- A - M - B = A - M - !C = A - M - 1 + C
        //   = A + (!M + 1) - 1 + C = A + !M + C (若和大于 255, 则不需要借位, Carry 为 1, 与加法处相同)
        self.add_to_a_with_carry_update_nvzc(!value); // 取负数并变补码
    }

    fn add_to_a_with_carry_update_nvzc(&mut self, value: u8) {
        let result = self.register_a as u16
            + value as u16
            + self.status.contains(CpuFlags::CARRY) as u16;

        // CARRY
        if result > 0xff {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        let result = result as u8;

        // OVERFLOW
        match (self.register_a >> 7, value >> 7, result >> 7){
            (1, 1, 0) | (0, 0, 1) => self.status.insert(CpuFlags::OVERFLOW),
            _ => self.status.remove(CpuFlags::OVERFLOW)
        }
        self.update_zero_and_negative_flags(result);
        self.register_a = result;
    }

    // fn and(&mut self, mode: &AddressingMode) {
    //     let addr = self.get_operand_address(mode);

    // }

    fn tax(&mut self) {
        self.register_x = self.register_a;

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn tay(&mut self) {
        self.register_y = self.register_a;

        self.update_zero_and_negative_flags(self.register_y);
    }

    fn tsx(&mut self) {
        self.register_x = self.stack_pointer;

        self.update_zero_and_negative_flags(self.register_x);
    }

    fn txa(&mut self) {
        self.register_a = self.register_x;

        self.update_zero_and_negative_flags(self.register_a);
    }

    fn txs(&mut self) {
        self.stack_pointer = self.register_x;
    }

    fn tya(&mut self) {
        self.register_a = self.register_y;

        self.update_zero_and_negative_flags(self.register_a);
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

    #[test]
    fn test_adc_add_2_bytes() {
        let mut cpu = CPU::new();
        // LDA $0x10; ADC $0x12; STA $0x14; LDA $0x11; ADC $0x13; STA $0x15; BRK
        cpu.load(vec![0xa5, 0x10, 0x65, 0x12, 0x85, 0x14,
            0xa5, 0x11, 0x65, 0x13, 0x85, 0x15, 0x00
            ]);
        cpu.reset();
        cpu.mem_write_u16(0x10, 0x01ff);
        cpu.mem_write_u16(0x12, 0x01);
        cpu.run();
        assert_eq!(cpu.mem_read_u16(0x14), 0x0200); // 0x01ff + 0x01 == 0x0200
    }

    #[test]
    fn test_sbc_5_minus_1() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0xa9, 0x5, // LDA #$0x5
            0xe9, 0x1, // SBC #$0x1
            0x00 // BRK
        ]);
        cpu.reset();
        cpu.status.insert(CpuFlags::CARRY); // Borrow bit is 0, so Carry is 1
        cpu.run();
        assert_eq!(cpu.register_a, 4);
    }

    #[test]
    fn test_sbc_1_minus_5() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0xa9, 0x1, // LDA #$0x5
            0xe9, 0x5, // SBC #$0x1
            0x00 // BRK
        ]);
        cpu.reset();
        cpu.status.insert(CpuFlags::CARRY); // Borrow bit is 0, so Carry is 1
        cpu.run();
        assert_eq!(cpu.register_a, -4i8 as u8);
    }

    #[test]
    fn test_sbc_sub_2_bytes() {
        let mut cpu = CPU::new();
        // LDA $0x10; SBC $0x12; STA $0x14; LDA $0x11; SBC $0x13; STA $0x15; BRK
        cpu.load(vec![
            0xa5, 0x10, 0xe5, 0x12, 0x85, 0x14,
            0xa5, 0x11, 0xe5, 0x13, 0x85, 0x15, 0x00
            ]);
        cpu.reset();
        cpu.status.insert(CpuFlags::CARRY); // Borrow bit is 0, so Carry is 1
        cpu.mem_write_u16(0x10, 0x0200);
        cpu.mem_write_u16(0x12, 0x01);
        cpu.run();
        assert_eq!(cpu.mem_read_u16(0x14), 0x01ff); // 0x0200 - 0x01 == 0x01ff
    }

    #[test]
    fn test_load_and_store() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0xa6, 0x10, // LDX $0x10 ; X <- *0x10
            0xac, 0x00, 0x02, // LDY $0x0200 ; Y <= *0x0200
            0xa1, 0x04, // LDA ($0x04, X) ; A <- **(X + 0x04)
            0x96, 0x02, // STX $0x02, Y ; *(0x02 + Y) <- X
            0x94, 0x02, // STY $0x02, X ; *(0x02 + X) <- Y
            0x91, 0x10, // STA ($0x10), Y ; *(*0x10 + Y) <- A
            0x00, // BRK
        ]);
        cpu.reset();
        cpu.mem_write(0x10, 0x14);
        cpu.mem_write(0x0200, 0x50);
        cpu.mem_write(0x18, 0x10);
        cpu.run();
        assert_eq!(cpu.register_x, 0x14);
        assert_eq!(cpu.register_y, 0x50);
        assert_eq!(cpu.register_a, 0x14);
        assert_eq!(cpu.mem_read(0x52), 0x14);
        assert_eq!(cpu.mem_read(0x16), 0x50);
        assert_eq!(cpu.mem_read(0x14 + 0x50), 0x14);
    }

    #[test]
    fn test_transfer() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x03, // LDA #$03
            0xa8, // TAY
            0xba, // TSX
            0x8a, // TXA
            0x00, // BRK
        ]);
        assert_eq!(cpu.register_y, 0x03);
        assert_eq!(cpu.register_x, cpu.stack_pointer);
        assert_eq!(cpu.register_a, cpu.register_x);
    }

    #[test]
    fn test_stack_push_pop() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x50, // LDA #$50
            0x48, // PHA
            0x08, // PHP
            0x68, // PLA
            0x00, // BRK
        ]);
        assert_eq!(cpu.mem_read(STACK + STACK_RESET as u16), 0x50);
        assert_eq!(cpu.mem_read(STACK + STACK_RESET as u16 - 1), cpu.register_a);
    }

    #[test]
    fn test_decrement_and_increment() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0xc6, 0x10, // DEC $10
            0xee, 0x00, 0x02,// INC $0200
            0xa2, 0x03, // LDX #$03
            0xca, // DEX
            0xa0, 0x04, // LDY #$04
            0xc8, // INY
            0x00, // BRK
        ]);
        cpu.reset();
        cpu.mem_write(0x10, 0x6);
        cpu.mem_write(0x0200, 0x6);
        cpu.run();
        assert_eq!(cpu.mem_read(0x10), 0x5);
        assert_eq!(cpu.mem_read(0x0200), 0x7);
        assert_eq!(cpu.register_x, 0x2);
        assert_eq!(cpu.register_y, 0x5);
    }

    #[test]
    fn test_asl() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0x0a, // ASL A
            0x06, 0x10, // ASL $10
            0x00, //BRK
        ]);
        cpu.reset();
        cpu.register_a = 0x03;
        cpu.mem_write(0x10, 0b1000_0000);
        cpu.run();
        assert_eq!(cpu.register_a, 0x06);
        assert_eq!(cpu.mem_read(0x10), 0x0);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_lsr() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0x4a, // LSR A
            0x46, 0x10, // LSR $10
            0x00, //BRK
        ]);
        cpu.reset();
        cpu.register_a = 0x03;
        cpu.mem_write(0x10, 0b1000_0000);
        cpu.run();
        assert_eq!(cpu.register_a, 0x01);
        assert_eq!(cpu.mem_read(0x10), 0b0100_0000);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_rol_ror() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0x66, 0x10, // ROR $10
            0x2e, 0x00, 0x02,  // ROL $0200
            0x00, // BRK
        ]);
        cpu.reset();
        cpu.mem_write(0x10, 0b0000_0011);
        cpu.mem_write(0x0200, 1);
        cpu.run();
        assert_eq!(cpu.mem_read(0x10), 1);
        assert_eq!(cpu.mem_read(0x0200), 0b0000_0011);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_and_ora_eor() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0xa9, 0x81, // LDA #$81 ; 0b1000_0001
            0x25, 0x10, // AND $10
            0x85, 0x11, // STA $11
            0xa9, 0x61, // LDA #$61 ; 0b0110_0001
            0x05, 0x10, // ORA $10
            0x85, 0x12, // STA $12
            0xa9, 0x68, // LDA #$69 ; 0b0110_1000
            0x45, 0x10, // EOR $10
            0x85, 0x13, // STA $13
            0x00, // BRK
        ]);
        cpu.reset();
        cpu.mem_write(0x10, 0b1001_0110);
        cpu.run();
        assert_eq!(cpu.mem_read(0x11), 0b1000_0000);
        assert_eq!(cpu.mem_read(0x12), 0b1111_0111);
        assert_eq!(cpu.mem_read(0x13), 0b1111_1110);
    }

    #[test]
    fn test_bit() {
        let mut cpu = CPU::new();
        cpu.load(vec![
            0x24, 0x10, // BIT $10
            0x00, // BRK
        ]);
        cpu.reset();
        cpu.register_a = 0b1000_1000;
        cpu.mem_write(0x10, 0b1100_0001);
        cpu.run();
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(cpu.status.contains(CpuFlags::OVERFLOW));
    }

    #[test]
    fn test_compare_1_8() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0x08, // CMP #$08
            0x00, // BRK
        ]);
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_8_1() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x08, // LDA #$08
            0xc9, 0x01, // CMP #$01
            0x00, // BRK
        ]);
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_1_1() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0x01, // CMP #$01
            0x00, // BRK
        ]);
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_1_255() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0xff, // CMP #$ff
            0x00, // BRK
        ]);
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_254_255() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0xfe, // LDA #$fe
            0xc9, 0xff, // CMP #$ff
            0x00, // BRK
        ]);
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_1_0() {
        let mut cpu = CPU::new();
        cpu.load_and_run(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0x00, // CMP #$00
            0x00, // BRK
        ]);
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }
}
