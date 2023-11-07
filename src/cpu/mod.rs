mod opcodes;
pub(crate) mod trace;

use bitflags::bitflags;
use crate::{bus::Bus, common::{Mem, Clock}, joypad::Joypad, apu::Samples, ppu::Frame, Rom};

use self::opcodes::OPCODES_MAP;

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
enum AddressingMode {
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
    /// - NMI 触发, UB=10, push 后 I 置为 1
    struct CpuFlags : u8 {
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

pub struct Cpu {
    // 组成
    register_a: u8,
    register_x: u8,
    register_y: u8,
    status: CpuFlags,
    program_counter: u16,
    stack_pointer: u8,  // 指向空位置
    bus: Bus, // 总线(连接CPU RAM, PPU, Rom 等)
    // 状态信息
    brk_flag: bool,
    prev_nmi_line_level: bool, // 上个周期的 nmi 线电平
    nmi_pending: bool, // nmi 是否正在 pending
    irq_pending: bool, // irq 是否正在 pending
    frame_end: bool, // 是否到达了帧末尾(直到下一条指令才会重置)
}

const STACK: u16 = 0x0100; // stack pointer + STACK 即为真正的栈指针
const STACK_RESET: u8 = 0xfd;
const INTERRUPT_RESET_VECTOR: u16 = 0xfffc;
const INTERRUPT_NMI_VECTOR: u16 = 0xfffa;
const INTERRUPT_IRQ_BRK_VECTOR: u16 = 0xfffe;

impl Mem for Cpu {
    fn mem_read(&mut self, addr: u16) -> u8 {
        self.bus.mem_read(addr)
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        self.bus.mem_write(addr, data);
    }

    fn mem_read_u16(&mut self, addr: u16) -> u16 {
        self.bus.mem_read_u16(addr)
    }

    fn mem_write_u16(&mut self, addr: u16, data: u16) {
        self.bus.mem_write_u16(addr, data);
    }
}

impl Cpu {
    /// create a new Cpu with a Rom
    pub fn new(rom: Rom) -> Self {
        Cpu {
            register_a: 0,
            register_x: 0,
            register_y: 0,
            status: CpuFlags::from_bits_truncate(0b100100),
            program_counter: 0,
            stack_pointer: STACK_RESET,
            bus: Bus::new(rom),
            brk_flag: false,
            prev_nmi_line_level: true,
            nmi_pending: false,
            irq_pending: false,
            frame_end: false,
        }
    }

    /// returns frame(video output), joypad(controller input) and samples(audio output)
    pub fn io_interface(&mut self) -> (&Frame, &mut Joypad, &mut Samples) {
        self.bus.io_interface()
    }

    /// run next frame
    pub fn run_next_frame(&mut self) {
        while !self.run_next_instruction() {}
    }

    /// run next instruction, returns true if this frame is end
    pub fn run_next_instruction(&mut self) -> bool {
        self.run_next_instruction_with_trace(|_| {})
    }

    /// run next frame, with a trace function called every instruction cycle
    pub fn run_next_frame_with_trace<F>(&mut self, mut trace: F)
    where
        F: FnMut(&mut Cpu)
    {
        while !self.run_next_instruction_with_trace(|cpu| trace(cpu)) { }
    }

    /// run next instruction, with a trace funtion called before execution, returns true if this frame is end
    pub fn run_next_instruction_with_trace<F>(&mut self, mut trace: F) -> bool 
    where
        F: FnMut(&mut Cpu)
    {
        self.frame_end = false;
        // trace
        trace(self);
        // 执行
        self.execute_instruction();
        // 处理中断
        if self.nmi_pending {
            self.nmi_pending = false;
            self.nmi();
        } else if self.irq_pending && !self.status.contains(CpuFlags::INTERRUPT_DISABLE) {
            self.irq_pending = false;
            self.irq();
        }
        self.frame_end
    } 

    /// 模拟 NES 插入卡带时的动作(RESET 中断)
    /// 1. 状态重置(寄存器与状态寄存器)
    /// 2. 将 PC 寄存器值设为地址 0xFFFC 处的 16 bit 数值
    pub fn reset(&mut self) {
        self.register_a = 0;
        self.register_x = 0;
        self.register_y = 0;
        self.status = CpuFlags::from_bits_truncate(0b100100);
        self.stack_pointer = STACK_RESET;

        self.program_counter = self.mem_read_u16(INTERRUPT_RESET_VECTOR);
    }

    /// NMI 中断
    /// 1. 下一条指令地址入栈
    /// 2. 状态寄存器入栈(UB=10)
    /// 3. 状态寄存器 I 置 1
    /// 4. 将 PC 寄存器值设为地址 0xFFFA 处的 16 bit 数值
    fn nmi(&mut self) {
        self.stack_push_u16(self.program_counter); // 下一条指令地址
        let mut flag = self.status.clone();
        flag.insert(CpuFlags::BREAK2);
        flag.remove(CpuFlags::BREAK);
        self.stack_push(flag.bits);
        self.status.insert(CpuFlags::INTERRUPT_DISABLE);

        self.clock();
        self.clock();
        self.program_counter = self.mem_read_u16(INTERRUPT_NMI_VECTOR);
    }

    /// IRQ 中断
    /// 1. 下一条指令地址入栈
    /// 2. 状态寄存器入栈(UB=10)
    /// 3. 状态寄存器 I 置 1
    /// 4. 将 PC 寄存器值设为地址 0xFFFE 处的 16 bit 数值
    fn irq(&mut self) {
        self.stack_push_u16(self.program_counter); // 下一条指令地址
        let mut flag = self.status.clone();
        flag.insert(CpuFlags::BREAK2);
        flag.remove(CpuFlags::BREAK);
        self.stack_push(flag.bits);
        self.status.insert(CpuFlags::INTERRUPT_DISABLE);

        self.clock();
        self.clock();
        self.program_counter = self.mem_read_u16(INTERRUPT_IRQ_BRK_VECTOR);
    }
}

impl Clock for Cpu {
    type Result = ();

    fn clock(&mut self) -> Self::Result {
        if self.bus.clock() {
            self.frame_end = true;
        }
        if self.prev_nmi_line_level && !self.bus.nmi_line_level() {
            self.nmi_pending = true;
        }
        self.prev_nmi_line_level = self.bus.nmi_line_level();
        self.irq_pending = !self.bus.irq_line_level();
    }
}

impl Cpu{
    /// CPU 执行一条指令
    fn execute_instruction(&mut self) {
        // 操作码解码
        let code = self.mem_read(self.program_counter);
        self.program_counter += 1;
        let program_counter_before = self.program_counter; // 用来标记是否发生了跳转
        let opcode = OPCODES_MAP.get(&code).expect(&format!("OpCode {:02x} is not recognized", code));

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
            // 跳转与返回
            0x4c => {
                self.jmp_absolute();
            }
            0x6c => {
                self.jmp_indirect();
            }
            0x20 => {
                self.jsr();
            }
            0x40 => {
                self.rti();
            }
            0x60 => {
                self.rts();
            }
            // 分支
            0x90 => { // BCC
                if !self.status.contains(CpuFlags::CARRY) {
                    self.branch();
                }
            }
            0xb0 => { // BCS
                if self.status.contains(CpuFlags::CARRY) {
                    self.branch();
                }
            }
            0xf0 => { // BEQ
                if self.status.contains(CpuFlags::ZERO) {
                    self.branch();
                }
            }
            0x30 => { // BMI
                if self.status.contains(CpuFlags::NEGATIVE) {
                    self.branch();
                }
            }
            0xd0 => { // BNE
                if !self.status.contains(CpuFlags::ZERO) {
                    self.branch();
                }
            }
            0x10 => { // BPL
                if !self.status.contains(CpuFlags::NEGATIVE) {
                    self.branch();
                }
            }
            0x50 => { // BVC
                if !self.status.contains(CpuFlags::OVERFLOW) {
                    self.branch();
                }
            }
            0x70 => { // BVS
                if self.status.contains(CpuFlags::OVERFLOW) {
                    self.branch();
                }
            }
            // 状态寄存器
            0x18 => {
                self.clc();
            }
            0xd8 => {
                self.cld();
            }
            0x58 => {
                self.cli();
            }
            0xb8 => {
                self.clv();
            }
            0x38 => {
                self.sec();
            }
            0xf8 => {
                self.sed();
            }
            0x78 => {
                self.sei();
            }
            // 传送指令
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
            0xea => { // nop
                // nothing
            }
            0x00 => { // BRK
                self.brk_flag = true;  // TODO: 软中断还未实现
            }
            // unofficial
            0x07 | 0x17 | 0x0f | 0x1f | 0x1b | 0x03 | 0x13 => {
                self.slo(&opcode.mode);
            }
            0x27 | 0x37 | 0x2f | 0x3f | 0x3b | 0x23 | 0x33 => {
                self.rla(&opcode.mode);
            }
            0x47 | 0x57 | 0x4f | 0x5f | 0x5b | 0x43 | 0x53 => {
                self.sre(&opcode.mode);
            }
            0x67 | 0x77 | 0x6f | 0x7f | 0x7b | 0x63 | 0x73 => {
                self.rra(&opcode.mode);
            }
            0x87 | 0x97 | 0x83 | 0x8f => {
                self.sax(&opcode.mode);
            }
            0xa7 | 0xb7 | 0xaf | 0xbf | 0xa3 | 0xb3 => {
                self.lax(&opcode.mode);
            }
            0xc7 | 0xd7 | 0xcf | 0xdf | 0xdb | 0xc3 | 0xd3 => {
                self.dcp(&opcode.mode);
            }
            0xe7 | 0xf7 | 0xef | 0xff | 0xfb | 0xe3 | 0xf3 => {
                self.isc(&opcode.mode);
            }
            0x0b | 0x2b => {
                self.anc(&opcode.mode);
            }
            0x4b => {
                self.alr(&opcode.mode);
            }
            0x6b => {
                self.arr(&opcode.mode);
            }
            0x8b => {
                self.xaa(&opcode.mode);
            }
            0xab => {
                self.lax(&opcode.mode);
            }
            0xcb => {
                self.axs(&opcode.mode);
            }
            0xeb => {
                self.sbc(&opcode.mode);
            }
            0x9f | 0x93 => {
                self.ahx(&opcode.mode);
            }
            0x9c => {
                self.shy(&opcode.mode);
            }
            0x9e => {
                self.shx(&opcode.mode);
            }
            0x9b => {
                self.tas(&opcode.mode);
            }
            0xbb => {
                self.las(&opcode.mode);
            }
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xb2 | 0xd2 | 0xf2 => { // KIL
                todo!("KIL todo");
            }
            _ => { // NOP, DOP, TOP
                // nothing
            }
        }

        if program_counter_before == self.program_counter { // 没有进行跳转则转至下一条指令
            self.program_counter += (opcode.len - 1) as u16;
        }

        for _ in 0..opcode.cycles {
            self.clock();
        }
    }

    fn get_operand_address(&mut self, mode: &AddressingMode) -> u16 {
        match mode {
            AddressingMode::Immediate => self.program_counter,
            _ => self.get_absolute_address(mode, self.program_counter),
        }
    }

    fn get_absolute_address(&mut self, mode: &AddressingMode, addr: u16) -> u16 {
        match mode {
            AddressingMode::ZeroPage => self.mem_read(addr) as u16,
            AddressingMode::ZeroPage_X => {
                let pos = self.mem_read(addr);
                pos.wrapping_add(self.register_x) as u16
            }
            AddressingMode::ZeroPage_Y => {
                let pos = self.mem_read(addr);
                pos.wrapping_add(self.register_y) as u16
            }
            AddressingMode::Absolute => self.mem_read_u16(addr),
            AddressingMode::Absolute_X => {
                let pos = self.mem_read_u16(addr);
                pos.wrapping_add(self.register_x as u16)
            }
            AddressingMode::Absolute_Y => {
                let pos = self.mem_read_u16(addr);
                pos.wrapping_add(self.register_y as u16)
            }
            AddressingMode::Indirect_X => {
                let base = self.mem_read(addr);
                let ptr = base.wrapping_add(self.register_x);
                let lo = self.mem_read(ptr as u16) as u16;
                let hi = self.mem_read(ptr.wrapping_add(1) as u16) as u16; // 不能超过 ZeroPage
                (hi << 8) | lo
            }
            AddressingMode::Indirect_Y => {
                let ptr = self.mem_read(addr);
                let lo = self.mem_read(ptr as u16) as u16;
                let hi = self.mem_read(ptr.wrapping_add(1) as u16) as u16; // 不能超过 ZeroPage
                let addr_base = (hi << 8) | lo;
                addr_base.wrapping_add(self.register_y as u16)
            }
            _ => {
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

    fn stack_push_u16(&mut self, data: u16) {
        let lo = (data & 0xff) as u8;
        let hi = (data >> 8) as u8;
        self.stack_push(hi);
        self.stack_push(lo);
    }

    fn stack_pop(&mut self) -> u8 {
        self.stack_pointer = self.stack_pointer.wrapping_add(1);
        self.mem_read(STACK + self.stack_pointer as u16)
    }

    fn stack_pop_u16(&mut self) -> u16 {
        let lo = self.stack_pop() as u16;
        let hi = self.stack_pop() as u16;
        (hi << 8) | lo
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

    fn jmp_absolute(&mut self) {
        self.program_counter = self.mem_read_u16(self.program_counter);
    }

    fn jmp_indirect(&mut self) {
        // 间接寻址不会超过页面, 而是回环
        let addr = self.mem_read_u16(self.program_counter);
        let target = if addr & 0x00ff == 0x00ff {
            let lo = self.mem_read(addr) as u16;
            let hi = self.mem_read(addr & 0xff00) as u16;
            (hi << 8) | lo
        } else {
            self.mem_read_u16(addr)
        };
        self.program_counter = target;
    }

    fn jsr(&mut self) {
        // pushes the address-1 of the next operation on to the stack
        let next_minus_1 = self.program_counter.wrapping_add(1);
        self.stack_push_u16(next_minus_1);
        self.program_counter = self.mem_read_u16(self.program_counter);
    }

    fn rts(&mut self) {
        let next_minus_1 = self.stack_pop_u16();
        self.program_counter = next_minus_1.wrapping_add(1);
    }

    fn rti(&mut self) {
        self.status.bits = self.stack_pop();
        self.status.insert(CpuFlags::BREAK2);
        self.status.remove(CpuFlags::BREAK);
        self.program_counter = self.stack_pop_u16();
    }

    fn branch(&mut self) {
        let offset = self.mem_read(self.program_counter) as i8; // branch 有符号
        self.program_counter = self.program_counter
            .wrapping_add(1)
            .wrapping_add(offset as u16);
    }

    fn clc(&mut self) {
        self.status.remove(CpuFlags::CARRY);
    }

    fn cld(&mut self) {
        self.status.remove(CpuFlags::DECIMAL);
    }

    fn cli(&mut self) {
        self.status.remove(CpuFlags::INTERRUPT_DISABLE);
    }

    fn clv(&mut self) {
        self.status.remove(CpuFlags::OVERFLOW);
    }

    fn sec(&mut self) {
        self.status.insert(CpuFlags::CARRY);
    }

    fn sed(&mut self) {
        self.status.insert(CpuFlags::DECIMAL);
    }

    fn sei(&mut self) {
        self.status.insert(CpuFlags::INTERRUPT_DISABLE);
    }

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

    // unofficial

    // Shift left one bit in memory, then OR accumulator with memory.
    fn slo(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.arithmetic_shift_left_update_nzc(data);
        self.mem_write(addr, data);
        self.register_a = self.register_a | data;
        self.update_zero_and_negative_flags(self.register_a);
    }

    // Rotate one bit left in memory, then AND accumulator with memory
    fn rla(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.rotate_left_through_carry_update_nzc(data);
        self.mem_write(addr, data);
        self.register_a = self.register_a & data;
        self.update_zero_and_negative_flags(self.register_a);
    }

    // Shift right one bit in memory, then EOR accumulator with memory.
    fn sre(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.logical_shift_right_update_nzc(data);
        self.mem_write(addr, data);
        self.register_a = self.register_a ^ data;
        self.update_zero_and_negative_flags(self.register_a);
    }

    // Rotate one bit right in memory, then add memory to accumulator (with carry).
    fn rra(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let data = self.rotate_right_through_carry_update_nzc(data);
        self.mem_write(addr, data);
        self.add_to_a_with_carry_update_nvzc(data);
    }

    // AND X register with accumulator and store result in memory.
    fn sax(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let result = self.register_a & self.register_x;
        self.mem_write(addr, result);
    }

    // Load accumulator and X register with memory.
    fn lax(&mut self, mode: &AddressingMode) {
        self.lda(mode);
        self.tax();
    }

    // Subtract 1 from memory (without borrow).
    // 通过 A - result 的结果改变 NZC
    fn dcp(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let result = self.mem_read(addr).wrapping_sub(1);
        self.mem_write(addr, result);

        if self.register_a >= result {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        self.update_zero_and_negative_flags(self.register_a.wrapping_sub(result));
    }

    // Increase memory by one, then subtract memory from accu-mulator (with borrow).
    fn isc(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let result = self.mem_read(addr).wrapping_add(1);
        self.mem_write(addr, result);

        // 原理见 fn sbc 注释
        self.add_to_a_with_carry_update_nvzc(!result);
    }

    // AND byte with accumulator. If result is negative then carry is set.
    fn anc(&mut self, mode: &AddressingMode) {
        self.and(mode);
        if self.status.contains(CpuFlags::NEGATIVE) {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }
    }

    // AND byte with accumulator, then shift right one bit in accumulator.
    fn alr(&mut self, mode: &AddressingMode) {
        self.and(mode);
        self.lsr_a();
    }

    // AND byte with accumulator, then rotate one bit right in accumulator and
    // check bit 5 and 6:
    // If both bits are 1: set C, clear V.
    // If both bits are 0: clear C and V.
    // If only bit 5 is 1: set V, clear C.
    // If only bit 6 is 1: set C and V.
    fn arr(&mut self, mode: &AddressingMode) {
        self.and(mode);
        self.ror_a();
        let bit5 = self.register_a & 0b0010_0000 == 0b0010_0000;
        let bit6 = self.register_a & 0b0100_0000 == 0b0100_0000;
        match (bit5, bit6) {
            (true, true) => {
                self.status.insert(CpuFlags::CARRY);
                self.status.remove(CpuFlags::OVERFLOW);
            }
            (false, false) => {
                self.status.remove(CpuFlags::CARRY);
                self.status.remove(CpuFlags::OVERFLOW);
            }
            (true, false) => {
                self.status.remove(CpuFlags::CARRY);
                self.status.insert(CpuFlags::OVERFLOW);
            }
            (false, true) => {
                self.status.insert(CpuFlags::CARRY);
                self.status.insert(CpuFlags::OVERFLOW);
            }
        }
    }

    // 	A := X & #{imm}
    fn xaa(&mut self, mode: &AddressingMode) {
        self.txa();
        self.and(mode);
    }

    // AND X register with accumulator and store result in X register, then subtract byte from X register (without borrow).
    fn axs(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        self.register_x = self.register_x & self.register_a;

        if self.register_x >= data {
            self.status.insert(CpuFlags::CARRY);
        } else {
            self.status.remove(CpuFlags::CARRY);
        }

        self.register_x = self.register_x.wrapping_sub(data);
        self.update_zero_and_negative_flags(self.register_x);
    }

    // {adr} := A & X & High(adr)
    fn ahx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let result = self.register_a & self.register_x & (addr >> 8) as u8;
        self.mem_write(addr, result);
    }

    // {adr} := Y & H
    fn shy(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let result = self.register_y & (addr >> 8) as u8;
        self.mem_write(addr, result);
    }

    // {adr} := X & H
    fn shx(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let result = self.register_x & (addr >> 8) as u8;
        self.mem_write(addr, result);
    }

    // AND X register with accumulator and store result in stack pointer, then AND stack pointer with the high byte of the target address of the argument + 1. Store result in memory.
    fn tas(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        self.stack_pointer = self.register_x & self.register_a;
        let result = self.stack_pointer & ((addr >> 8) as u8 + 1);
        self.mem_write(addr, result);
    }

    // A,X,S:={adr}&S
    fn las(&mut self, mode: &AddressingMode) {
        let addr = self.get_operand_address(mode);
        let data = self.mem_read(addr);
        let result = data & self.stack_pointer;
        self.update_zero_and_negative_flags(result);
        self.register_a = result;
        self.register_x = result;
        self.stack_pointer = result;
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::tests::*;

    impl Cpu {
        fn run_until_brk(&mut self) {
            while !self.brk_flag {
                self.run_next_instruction();
            }
        }
    }

    #[test]
    fn test_0xa9_lda_immidiate_load_data() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xa9, 0x05, 0x00])); // LDA #$05; BRK
        cpu.reset();
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, 0x05);
        assert!(cpu.status.bits() & 0b0000_0010 == 0b00); // Z is 0
        assert!(cpu.status.bits() & 0b1000_0000 == 0b00); // N is 0
    }

    #[test]
    fn test_0xa9_lda_zero_flag() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xa9, 0x00, 0x00])); // LDA #$00; BRK
        cpu.reset();
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, 0x00);
        assert!(cpu.status.bits() & 0b0000_0010 == 0b10); // Z is 1
    }

    #[test]
    fn test_0xa9_lda_neg() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xa9, 0xff, 0x00])); // LDA #$FF; BRK
        cpu.reset();
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, 0xff);
        assert!(cpu.status.bits() & 0b1000_0000 == 0b1000_0000); // N is 1
    }

    #[test]
    fn test_0xaa_tax_move_a_to_x() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xaa, 0x00])); // TAX; BRK
        cpu.reset();
        cpu.register_a = 10;
        cpu.run_until_brk();

        assert_eq!(cpu.register_x, 10)
    }

    #[test]
    fn test_lda_from_memory() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xa5, 0x10, 0x00])); // LDA $10; BRK
        cpu.reset();
        cpu.mem_write(0x10, 0x55);
        cpu.run_until_brk();

        assert_eq!(cpu.register_a, 0x55);
    }

    #[test]
    fn test_5_ops_working_together() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0xc0, // LDA #$c0
            0xaa, 0xe8, 0x00 // TAX; INX; BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();

        assert_eq!(cpu.register_x, 0xc1)
    }

    #[test]
    fn test_inx_overflow() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xe8, 0xe8, 0x00, // INX; INX; BRK
        ]));
        cpu.reset();
        cpu.register_x = 0xff;
        cpu.run_until_brk();

        assert_eq!(cpu.register_x, 1)
    }

    #[test]
    fn test_adc_add_2_bytes() {
        // LDA $0x10; ADC $0x12; STA $0x14; LDA $0x11; ADC $0x13; STA $0x15; BRK
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa5, 0x10, 0x65, 0x12, 0x85, 0x14,
            0xa5, 0x11, 0x65, 0x13, 0x85, 0x15, 0x00,
        ]));
        cpu.reset();
        cpu.mem_write_u16(0x10, 0x01ff);
        cpu.mem_write_u16(0x12, 0x01);
        cpu.run_until_brk();
        assert_eq!(cpu.mem_read_u16(0x14), 0x0200); // 0x01ff + 0x01 == 0x0200
    }

    #[test]
    fn test_sbc_5_minus_1() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x5, // LDA #$0x5
            0xe9, 0x1, // SBC #$0x1
            0x00 // BRK
        ]));
        cpu.reset();
        cpu.status.insert(CpuFlags::CARRY); // Borrow bit is 0, so Carry is 1
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, 4);
    }

    #[test]
    fn test_sbc_1_minus_5() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x1, // LDA #$0x5
            0xe9, 0x5, // SBC #$0x1
            0x00 // BRK
        ]));
        cpu.reset();
        cpu.status.insert(CpuFlags::CARRY); // Borrow bit is 0, so Carry is 1
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, -4i8 as u8);
    }

    #[test]
    fn test_sbc_sub_2_bytes() {
        // LDA $0x10; SBC $0x12; STA $0x14; LDA $0x11; SBC $0x13; STA $0x15; BRK
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa5, 0x10, 0xe5, 0x12, 0x85, 0x14,
            0xa5, 0x11, 0xe5, 0x13, 0x85, 0x15, 0x00
        ]));
        cpu.reset();
        cpu.status.insert(CpuFlags::CARRY); // Borrow bit is 0, so Carry is 1
        cpu.mem_write_u16(0x10, 0x0200);
        cpu.mem_write_u16(0x12, 0x01);
        cpu.run_until_brk();
        assert_eq!(cpu.mem_read_u16(0x14), 0x01ff); // 0x0200 - 0x01 == 0x01ff
    }

    #[test]
    fn test_load_and_store() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa6, 0x10, // LDX $0x10 ; X <- *0x10
            0xac, 0x00, 0x02, // LDY $0x0200 ; Y <= *0x0200
            0xa1, 0x04, // LDA ($0x04, X) ; A <- **(X + 0x04)
            0x96, 0x02, // STX $0x02, Y ; *(0x02 + Y) <- X
            0x94, 0x02, // STY $0x02, X ; *(0x02 + X) <- Y
            0x91, 0x10, // STA ($0x10), Y ; *(*0x10 + Y) <- A
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.mem_write(0x10, 0x14);
        cpu.mem_write(0x0200, 0x50);
        cpu.mem_write(0x18, 0x10);
        cpu.run_until_brk();
        assert_eq!(cpu.register_x, 0x14);
        assert_eq!(cpu.register_y, 0x50);
        assert_eq!(cpu.register_a, 0x14);
        assert_eq!(cpu.mem_read(0x52), 0x14);
        assert_eq!(cpu.mem_read(0x16), 0x50);
        assert_eq!(cpu.mem_read(0x14 + 0x50), 0x14);
    }

    #[test]
    fn test_transfer() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x03, // LDA #$03
            0xa8, // TAY
            0xba, // TSX
            0x8a, // TXA
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert_eq!(cpu.register_y, 0x03);
        assert_eq!(cpu.register_x, cpu.stack_pointer);
        assert_eq!(cpu.register_a, cpu.register_x);
    }

    #[test]
    fn test_stack_push_pop() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x50, // LDA #$50
            0x48, // PHA
            0x08, // PHP
            0x68, // PLA
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert_eq!(cpu.mem_read(STACK + STACK_RESET as u16), 0x50);
        assert_eq!(cpu.mem_read(STACK + STACK_RESET as u16 - 1), cpu.register_a);
    }

    #[test]
    fn test_decrement_and_increment() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xc6, 0x10, // DEC $10
            0xee, 0x00, 0x02,// INC $0200
            0xa2, 0x03, // LDX #$03
            0xca, // DEX
            0xa0, 0x04, // LDY #$04
            0xc8, // INY
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.mem_write(0x10, 0x6);
        cpu.mem_write(0x0200, 0x6);
        cpu.run_until_brk();
        assert_eq!(cpu.mem_read(0x10), 0x5);
        assert_eq!(cpu.mem_read(0x0200), 0x7);
        assert_eq!(cpu.register_x, 0x2);
        assert_eq!(cpu.register_y, 0x5);
    }

    #[test]
    fn test_asl() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0x0a, // ASL A
            0x06, 0x10, // ASL $10
            0x00, //BRK
        ]));
        cpu.reset();
        cpu.register_a = 0x03;
        cpu.mem_write(0x10, 0b1000_0000);
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, 0x06);
        assert_eq!(cpu.mem_read(0x10), 0x0);
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_lsr() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0x4a, // LSR A
            0x46, 0x10, // LSR $10
            0x00, //BRK
        ]));
        cpu.reset();
        cpu.register_a = 0x03;
        cpu.mem_write(0x10, 0b1000_0000);
        cpu.run_until_brk();
        assert_eq!(cpu.register_a, 0x01);
        assert_eq!(cpu.mem_read(0x10), 0b0100_0000);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_rol_ror() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0x66, 0x10, // ROR $10
            0x2e, 0x00, 0x02,  // ROL $0200
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.mem_write(0x10, 0b0000_0011);
        cpu.mem_write(0x0200, 1);
        cpu.run_until_brk();
        assert_eq!(cpu.mem_read(0x10), 1);
        assert_eq!(cpu.mem_read(0x0200), 0b0000_0011);
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_and_ora_eor() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
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
        ]));
        cpu.reset();
        cpu.mem_write(0x10, 0b1001_0110);
        cpu.run_until_brk();
        assert_eq!(cpu.mem_read(0x11), 0b1000_0000);
        assert_eq!(cpu.mem_read(0x12), 0b1111_0111);
        assert_eq!(cpu.mem_read(0x13), 0b1111_1110);
    }

    #[test]
    fn test_bit() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0x24, 0x10, // BIT $10
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.register_a = 0b1000_1000;
        cpu.mem_write(0x10, 0b1100_0001);
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(cpu.status.contains(CpuFlags::OVERFLOW));
    }

    #[test]
    fn test_compare_1_8() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0x08, // CMP #$08
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_8_1() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x08, // LDA #$08
            0xc9, 0x01, // CMP #$01
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_1_1() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0x01, // CMP #$01
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_1_255() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0xff, // CMP #$ff
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_254_255() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0xfe, // LDA #$fe
            0xc9, 0xff, // CMP #$ff
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert!(cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(!cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_compare_1_0() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![
            0xa9, 0x01, // LDA #$01
            0xc9, 0x00, // CMP #$00
            0x00, // BRK
        ]));
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::NEGATIVE));
        assert!(!cpu.status.contains(CpuFlags::ZERO));
        assert!(cpu.status.contains(CpuFlags::CARRY));
    }

    #[test]
    fn test_status() {
        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0x18, 0x00])); // CLC
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::CARRY));

        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xd8, 0x00])); // CLD
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::DECIMAL));

        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0x58, 0x00])); // CLI
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::INTERRUPT_DISABLE));

        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xb8, 0x00])); // CLV
        cpu.reset();
        cpu.run_until_brk();
        assert!(!cpu.status.contains(CpuFlags::OVERFLOW));

        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0x38, 0x00])); // SEC
        cpu.reset();
        cpu.run_until_brk();
        assert!(cpu.status.contains(CpuFlags::CARRY));

        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0xf8, 0x00])); // SED
        cpu.reset();
        cpu.run_until_brk();
        assert!(cpu.status.contains(CpuFlags::DECIMAL));

        let mut cpu = Cpu::new(test_rom_with_2_bank_prg(vec![0x78, 0x00])); // SEI
        cpu.reset();
        cpu.run_until_brk();
        assert!(cpu.status.contains(CpuFlags::INTERRUPT_DISABLE));
    }
}
