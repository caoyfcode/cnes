/// # CPU struct
/// `status`: NV-BDIZC(Negative, Overflow, Break, Decimal, Interrupt Disable, Zero, Carry)
pub struct CPU {
    pub register_a: u8,
    pub status: u8,
    pub program_counter: u16,
}

impl CPU {
    pub fn new() -> Self {
        CPU {
            register_a: 0,
            status: 0,
            program_counter: 0,
        }
    }

    pub fn interpret(&mut self, program: Vec<u8>) {
        self.program_counter = 0;
        loop {
            let opcode = program[self.program_counter as usize];
            self.program_counter += 1;

            match opcode {
                // mode, syntax, len, time, flags
                0xA9 => { // Immediate, LDA #$44, 2, 2, NZ
                    let param = program[self.program_counter as usize];
                    self.program_counter += 1;
                    self.register_a = param;

                    if self.register_a == 0 { // Z
                        self.status = self.status | 0b0000_0010;
                    } else {
                        self.status = self.status & 0b1111_1101;
                    }

                    if self.register_a & 0b1000_0000 != 0 { // N
                        self.status = self.status | 0b1000_0000;
                    } else {
                        self.status = self.status & 0b0111_1111;
                    }
                }
                0x00 => { // Implied, BRK, 1, 7
                    return;  // just end
                }
                _ => todo!()
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_0xa9_lda_immidiate_load_data() {
        let mut cpu = CPU::new();
        cpu.interpret(vec![0xa9, 0x05, 0x00]); // LDA #$05; BRK
        assert_eq!(cpu.register_a, 0x05);
        assert!(cpu.status & 0b0000_0010 == 0b00); // Z is 0
        assert!(cpu.status & 0b1000_0000 == 0b00); // N is 0
    }

    #[test]
    fn test_0xa9_lda_zero_flag() {
        let mut cpu = CPU::new();
        cpu.interpret(vec![0xa9, 0x00, 0x00]); // LDA #$00; BRK
        assert_eq!(cpu.register_a, 0x00);
        assert!(cpu.status & 0b0000_0010 == 0b10); // Z is 1
    }

    #[test]
    fn test_0xa9_lda_neg() {
        let mut cpu = CPU::new();
        cpu.interpret(vec![0xa9, 0xff, 0x00]); // LDA #$FF; BRK
        assert_eq!(cpu.register_a, 0xff);
        assert!(cpu.status & 0b1000_0000 == 0b1000_0000); // N is 1
    }
}
