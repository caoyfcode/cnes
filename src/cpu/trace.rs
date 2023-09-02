use std::collections::HashMap;

use super::{opcodes, Cpu, Mem, AddressingMode};

/// 得到 cpu 下一条要执行的指令信息, 在该指令执行前调用
#[cfg(test)]
fn trace(cpu: &mut Cpu) -> String {
    let ref opcodes: HashMap<u8, &'static opcodes::OpCode> = *opcodes::OPCODES_MAP;
    let code = cpu.mem_read(cpu.program_counter);
    let opcode = opcodes.get(&code).expect(&format!("OpCode {:02x} is not recognized", code));

    let mut hex_dump = vec![code];

    let (mem_addr, mem_val) = match opcode.mode {
        AddressingMode::Immediate | AddressingMode::NoneAddressing => (0, 0),
        _ => {
            let addr = cpu.get_absolute_address(&opcode.mode, cpu.program_counter + 1);
            (addr, cpu.mem_read(addr))
        }
    };
    let asm_operand_and_addr_val = match opcode.len {
        1 => match opcode.code {
            0x0a | 0x4a | 0x2a | 0x6a => format!("A "), // ASL, LSR, ROL, ROR, Accumulator mode
            _ => format!(""),
        }
        2 => {
            let operand = cpu.mem_read(cpu.program_counter + 1);
            hex_dump.push(operand);

            match opcode.mode {
                AddressingMode::Immediate => format!("#${:02x}", operand),
                AddressingMode::ZeroPage => format!("${:02x} = {:02x}", mem_addr, mem_val),
                AddressingMode::ZeroPage_X => format!(
                    "${:02x},X @ {:02x} = {:02x}",
                    operand, mem_addr, mem_val
                ),
                AddressingMode::ZeroPage_Y => format!(
                    "${:02x},Y @ {:02x} = {:02x}",
                    operand, mem_addr, mem_val
                ),
                AddressingMode::Indirect_X => format!(
                    "(${:02x},X) @ {:02x} = {:04x} = {:02x}",
                    operand,
                    operand.wrapping_add(cpu.register_x),
                    mem_addr,
                    mem_val
                ),
                AddressingMode::Indirect_Y => format!(
                    "(${:02x}),Y = {:04x} @ {:04x} = {:02x}",
                    operand,
                    mem_addr.wrapping_sub(cpu.register_y as u16),
                    mem_addr,
                    mem_val
                ),
                AddressingMode::NoneAddressing => { // branch 指令
                    let addr = cpu.program_counter
                        .wrapping_add(2)
                        .wrapping_add((operand as i8) as u16);
                    format!("${:04x}", addr)
                }
                _ => panic!(
                    "unexpected addressing mode {:?} has ops-len 2. code {:02x}",
                    opcode.mode, opcode.code
                ),
            }
        }
        3 => {
            let lo = cpu.mem_read(cpu.program_counter + 1);
            let hi = cpu.mem_read(cpu.program_counter + 2);
            hex_dump.push(lo);
            hex_dump.push(hi);

            let operand = cpu.mem_read_u16(cpu.program_counter + 1);

            match opcode.mode {
                AddressingMode::Absolute => format!("${:04x} = {:02x}", mem_addr, mem_val),
                AddressingMode::Absolute_X => format!(
                    "${:04x},X @ {:04x} = {:02x}",
                    operand, mem_addr, mem_val
                ),
                AddressingMode::Absolute_Y => format!(
                    "${:04x},Y @ {:04x} = {:02x}",
                    operand, mem_addr, mem_val
                ),
                AddressingMode::NoneAddressing => { // jump 指令
                    if opcode.code == 0x6c { // jump indirect
                        let target = if operand & 0x00ff == 0x00ff {
                            let lo = cpu.mem_read(operand) as u16;
                            let hi = cpu.mem_read(operand & 0xff00) as u16;
                            (hi << 8) | lo
                        } else {
                            cpu.mem_read_u16(operand)
                        };
                        format!("(${:04x}) = {:04x}", operand, target)
                    } else { // jmp absolute or jsr(absolute)
                        format!("${:04x}", operand)
                    }
                }
                _ => panic!(
                    "unexpected addressing mode {:?} has ops-len 3. code {:02x}",
                    opcode.mode, opcode.code
                )
            }
        }
        _ => { // 目前暂无
            format!("")
        }
    };
    let hex_str = hex_dump
        .iter()
        .map(|num| format!("{:02x}", num))
        .collect::<Vec<String>>()
        .join(" ");
    let asm_str = format!(
        "{:04x}  {:8} {: >4} {}",
        cpu.program_counter, hex_str, opcode.mnemonic, asm_operand_and_addr_val
    ).trim().to_string();

    format!(
        "{:47} A:{:02x} X:{:02x} Y:{:02x} P:{:02x} SP:{:02x}",
        asm_str, cpu.register_a, cpu.register_x, cpu.register_y, cpu.status, cpu.stack_pointer
    ).to_ascii_uppercase()
}

// 不显示 mem_val (可以避免读 PPU 寄存器导致状态改变)
/// a trace function, returns information of next instruction to be executed
pub fn trace_readonly(cpu: &mut Cpu) -> String {
    let ref opcodes: HashMap<u8, &'static opcodes::OpCode> = *opcodes::OPCODES_MAP;
    let code = cpu.mem_read(cpu.program_counter);
    let opcode = opcodes.get(&code).expect(&format!("OpCode {:02x} is not recognized", code));

    let mut hex_dump = vec![code];

    let mem_addr = match opcode.mode {
        AddressingMode::Immediate | AddressingMode::NoneAddressing => 0,
        _ => cpu.get_absolute_address(&opcode.mode, cpu.program_counter + 1),
    };
    let asm_operand_and_addr_val = match opcode.len {
        1 => match opcode.code {
            0x0a | 0x4a | 0x2a | 0x6a => format!("A "), // ASL, LSR, ROL, ROR, Accumulator mode
            _ => format!(""),
        }
        2 => {
            let operand = cpu.mem_read(cpu.program_counter + 1);
            hex_dump.push(operand);

            match opcode.mode {
                AddressingMode::Immediate => format!("#${:02x}", operand),
                AddressingMode::ZeroPage => format!("${:02x}", mem_addr),
                AddressingMode::ZeroPage_X => format!(
                    "${:02x},X @ {:02x}",
                    operand, mem_addr
                ),
                AddressingMode::ZeroPage_Y => format!(
                    "${:02x},Y @ {:02x}",
                    operand, mem_addr
                ),
                AddressingMode::Indirect_X => format!(
                    "(${:02x},X) @ {:02x} = {:04x}",
                    operand,
                    operand.wrapping_add(cpu.register_x),
                    mem_addr
                ),
                AddressingMode::Indirect_Y => format!(
                    "(${:02x}),Y = {:04x} @ {:04x}",
                    operand,
                    mem_addr.wrapping_sub(cpu.register_y as u16),
                    mem_addr
                ),
                AddressingMode::NoneAddressing => { // branch 指令
                    let addr = cpu.program_counter
                        .wrapping_add(2)
                        .wrapping_add((operand as i8) as u16);
                    format!("${:04x}", addr)
                }
                _ => panic!(
                    "unexpected addressing mode {:?} has ops-len 2. code {:02x}",
                    opcode.mode, opcode.code
                ),
            }
        }
        3 => {
            let lo = cpu.mem_read(cpu.program_counter + 1);
            let hi = cpu.mem_read(cpu.program_counter + 2);
            hex_dump.push(lo);
            hex_dump.push(hi);

            let operand = cpu.mem_read_u16(cpu.program_counter + 1);

            match opcode.mode {
                AddressingMode::Absolute => format!("${:04x}", mem_addr),
                AddressingMode::Absolute_X => format!(
                    "${:04x},X @ {:04x}",
                    operand, mem_addr
                ),
                AddressingMode::Absolute_Y => format!(
                    "${:04x},Y @ {:04x}",
                    operand, mem_addr
                ),
                AddressingMode::NoneAddressing => { // jump 指令
                    if opcode.code == 0x6c { // jump indirect
                        let target = if operand & 0x00ff == 0x00ff {
                            let lo = cpu.mem_read(operand) as u16;
                            let hi = cpu.mem_read(operand & 0xff00) as u16;
                            (hi << 8) | lo
                        } else {
                            cpu.mem_read_u16(operand)
                        };
                        format!("(${:04x}) = {:04x}", operand, target)
                    } else { // jmp absolute or jsr(absolute)
                        format!("${:04x}", operand)
                    }
                }
                _ => panic!(
                    "unexpected addressing mode {:?} has ops-len 3. code {:02x}",
                    opcode.mode, opcode.code
                )
            }
        }
        _ => { // 目前暂无
            format!("")
        }
    };
    let hex_str = hex_dump
        .iter()
        .map(|num| format!("{:02x}", num))
        .collect::<Vec<String>>()
        .join(" ");
    let asm_str = format!(
        "{:04x}  {:8} {: >4} {}",
        cpu.program_counter, hex_str, opcode.mnemonic, asm_operand_and_addr_val
    ).trim().to_string();

    format!(
        "{:47} A:{:02x} X:{:02x} Y:{:02x} P:{:02x} SP:{:02x}",
        asm_str, cpu.register_a, cpu.register_x, cpu.register_y, cpu.status, cpu.stack_pointer
    ).to_ascii_uppercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cartridge::tests::test_rom;

    impl Cpu {
        pub fn run_with_trace_until_brk<F>(&mut self, mut trace: F)
        where
            F: FnMut(&mut Cpu)
        {
            while !self.brk_flag {
                self.run_next_instruction_with_trace(|cpu| trace(cpu));
            }
        }
    }

    #[test]
    fn test_format_trace() {
        let mut cpu = Cpu::new(test_rom());
        cpu.mem_write(100, 0xa2);
        cpu.mem_write(101, 0x01);
        cpu.mem_write(102, 0xca);
        cpu.mem_write(103, 0x88);
        cpu.mem_write(104, 0x00);

        cpu.program_counter = 0x64;
        cpu.register_a = 1;
        cpu.register_x = 2;
        cpu.register_y = 3;
        let mut result: Vec<String> = vec![];
        cpu.run_with_trace_until_brk(|cpu| {
            result.push(trace(cpu));
        });
        assert_eq!(
            "0064  A2 01     LDX #$01                        A:01 X:02 Y:03 P:24 SP:FD",
            result[0]
        );
        assert_eq!(
            "0066  CA        DEX                             A:01 X:01 Y:03 P:24 SP:FD",
            result[1]
        );
        assert_eq!(
            "0067  88        DEY                             A:01 X:00 Y:03 P:26 SP:FD",
            result[2]
        );
    }

    #[test]
    fn test_format_mem_access() {
        let mut cpu = Cpu::new(test_rom());
        // ORA ($33), Y
        cpu.mem_write(100, 0x11);
        cpu.mem_write(101, 0x33);

        //data
        cpu.mem_write(0x33, 00);
        cpu.mem_write(0x34, 04);

        //target cell
        cpu.mem_write(0x400, 0xAA);

        cpu.program_counter = 0x64;
        cpu.register_y = 0;
        let mut result: Vec<String> = vec![];
        cpu.run_with_trace_until_brk(|cpu| {
            result.push(trace(cpu));
        });
        assert_eq!(
            "0064  11 33     ORA ($33),Y = 0400 @ 0400 = AA  A:00 X:00 Y:00 P:24 SP:FD",
            result[0]
        );
    }
}