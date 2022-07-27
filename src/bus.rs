use crate::cpu::Mem;

pub struct Bus {
    cpu_vram: [u8; 2048],  // 2KB CPU VRAM
}

impl Bus {
    pub fn new() -> Self {
        Bus {
            cpu_vram: [0; 2048]
        }
    }
}

const RAM: u16 = 0x0000; // RAM 起始地址
const RAM_MIRRORS_END: u16 = 0x1fff; // RAM 映射截止
const PPU_REGISTERS: u16 = 0x2000; // PPU Registers 起始地址
const PPU_REGISTERS_MIRRORS_END: u16 = 0x3fff; // PPU Registers 映射截止

impl Mem for Bus {
    fn mem_read(&self, addr: u16) -> u8 {
        match addr {
            RAM..=RAM_MIRRORS_END => { // CPU VRAM
                let mirror_down_addr = addr & 0b0000_0111_1111_1111;  // 0x0000..0x0800 为 RAM
                self.cpu_vram[mirror_down_addr as usize]
            }
            PPU_REGISTERS..=PPU_REGISTERS_MIRRORS_END => { // PPU Registers
                let _mirror_down_addr = addr & 0b0010_0000_0000_0111; // 0x2000..0x2008 为 PPU Registers
                todo!("PPU is not supported yet")
            }
            _ => {
                println!("Ignoring mem access at {}", addr);
                0
            }
        }
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        match addr {
            RAM..=RAM_MIRRORS_END => { // CPU VRAM
                let mirror_down_addr = addr & 0b0000_0111_1111_1111;  // 0x0000..0x0800 为 RAM
                self.cpu_vram[mirror_down_addr as usize] = data;
            }
            PPU_REGISTERS..=PPU_REGISTERS_MIRRORS_END => { // I/O Registers
                let _mirror_down_addr = addr & 0b0010_0000_0000_0111; // 0x2000..0x2008 为I/O Registers
                todo!("PPU is not supported yet")
            }
            _ => {
                println!("Ignoring mem access at {}", addr);
            }
        }
    }
}