use crate::{cartridge::Rom, ppu::PPU, joypad::Joypad, common::Mem};

// CPU memory map
//  _______________ $10000  _______________
// | PRG-ROM       |       |               |
// | Upper Bank    |       |               |
// |_ _ _ _ _ _ _ _| $C000 | PRG-ROM       |
// | PRG-ROM       |       |               |
// | Lower Bank    |       |               |
// |_______________| $8000 |_______________|
// | SRAM          |       | SRAM          |
// |_______________| $6000 |_______________|
// | Expansion ROM |       | Expansion ROM |
// |_______________| $4020 |_______________|
// | I/O Registers |       |               |
// |_ _ _ _ _ _ _ _| $4000 |               |
// | Mirrors       |       | I/O Registers |
// | $2000-$2007   |       |               |
// |_ _ _ _ _ _ _ _| $2008 |               |
// | I/O Registers |       |               |
// |_______________| $2000 |_______________|
// | Mirrors       |       |               |
// | $0000-$07FF   |       |               |
// |_ _ _ _ _ _ _ _| $0800 |               |
// | RAM           |       | RAM           |
// |_ _ _ _ _ _ _ _| $0200 |               |
// | Stack         |       |               |
// |_ _ _ _ _ _ _ _| $0100 |               |
// | Zero Page     |       |               |
// |_______________| $0000 |_______________|
// PPU registers:
// Controller: 0x2000 (Control 1)
// Mask:       0x2001 (Control 2)
// Status:     0x2002
// OAM Address:0x2003
// OAM Data:   0x2004
// Scroll:     0x2005
// Address:    0x2006
// Data:       0x2007
// OAM DMA:    0x4014

pub struct Bus<'call> {
    // 组成
    cpu_vram: [u8; 2048],  // 2KB CPU VRAM
    prg_rom: Vec<u8>,
    ppu: PPU,
    joypad: Joypad,
    // 状态信息
    cycles: u32, // CPU 时钟周期
    frame_callback: Box<dyn FnMut(&PPU, &mut Joypad) + 'call>
}

impl<'a> Bus<'a> {
    pub fn new(rom: Rom) -> Self {
        Self::new_with_frame_callback(rom, move |_, _| {})
    }

    pub fn new_with_frame_callback<'call, F>(rom: Rom, frame_callback: F) -> Bus<'call>
    where
        F: FnMut(&PPU, &mut Joypad) + 'call,
    {
        Bus {
            cpu_vram: [0; 2048],
            prg_rom: rom.prg_rom,
            ppu: PPU::new(rom.chr_rom, rom.screen_mirroring),
            joypad: Joypad::new(),
            cycles: 0,
            frame_callback: Box::from(frame_callback),
        }
    }

    pub fn tick(&mut self, cycles: u8) { // CPU 时钟经过 cycles 个周期
        self.cycles += cycles as u32;

        let nmi_before = self.ppu.nmi_interrupt().is_some();
        self.ppu.tick(3 * cycles);
        let nmi_after = self.ppu.nmi_interrupt().is_some();

        if !nmi_before && nmi_after {
            (self.frame_callback)(&self.ppu, &mut self.joypad);
        }
    }

    // 是否有 NMI 中断传来
    pub fn poll_nmi_status(&mut self) -> Option<u8> {
        self.ppu.poll_nmi_interrupt()
    }

    fn read_prg_rom(&self, addr: u16) -> u8 {
        let mut idx = addr - 0x8000;
        if self.prg_rom.len() == 0x4000 && idx >= 0x4000 { // 仅仅有 lower bank
            idx = idx % 0x4000;
        }
        self.prg_rom[idx as usize]
    }
}

impl Mem for Bus<'_> {
    fn mem_read(&mut self, addr: u16) -> u8 {
        match addr {
            0..=0x1fff => { // CPU VRAM
                let mirror_down_addr = addr & 0b0000_0111_1111_1111;  // 0x0000..0x0800 为 RAM
                self.cpu_vram[mirror_down_addr as usize]
            }
            0x2000 | 0x2001 | 0x2003 | 0x2005 | 0x2006 | 0x4014 => {
                println!("Attempt to read from write-only PPU address {:04x}", addr);
                0
            }
            0x2002 => self.ppu.read_status(),
            0x2004 => self.ppu.read_oam_data(),
            0x2007 => self.ppu.read_data(),
            0x2008..=0x3fff => { // PPU Registers
                let mirror_down_addr = addr & 0b0010_0000_0000_0111; // 0x2000..0x2008 为 PPU Registers
                self.mem_read(mirror_down_addr)
            }
            0x4000..=0x4013 | 0x4015 => {
                println!("Attempt to read from APU Register at {:04x}", addr);
                0
            }
            0x4016 => {
                self.joypad.read()
            }
            0x4017 => {
                println!("Ignoring joypad 2");
                0
            }
            0x8000..=0xffff => { // PRG ROM
                self.read_prg_rom(addr)
            }
            _ => {
                println!("Ignoring mem access at {:04x}", addr);
                0
            }
        }
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        match addr {
            0..=0x1fff => { // CPU VRAM
                let mirror_down_addr = addr & 0b0000_0111_1111_1111;  // 0x0000..0x0800 为 RAM
                self.cpu_vram[mirror_down_addr as usize] = data;
            }
            0x2000 => self.ppu.write_to_controller(data),
            0x2001 => self.ppu.write_to_mask(data),
            0x2002 => {
                println!("Attempt to read from read-only PPU address {:04x}", addr);
            }
            0x2003 => self.ppu.write_to_oam_addr(data),
            0x2004 => self.ppu.write_to_oam_data(data),
            0x2005 => self.ppu.write_to_scroll(data),
            0x2006 => self.ppu.write_to_addr(data),
            0x2007 => self.ppu.write_to_data(data),
            0x2008..=0x3fff => { // I/O Registers
                let mirror_down_addr = addr & 0b0010_0000_0000_0111; // 0x2000..0x2008 为I/O Registers
                self.mem_write(mirror_down_addr, data);
            }
            0x4014 => { // Writing $XX will upload 256 bytes of data from CPU page $XX00-$XXFF to the internal PPU OAM.
                let mut buffer: [u8; 256] = [0; 256];
                let base = (data as u16) << 8;
                for i in 0..=0xffu16 {
                    buffer[i as usize] = self.mem_read(base + i);
                }
                self.ppu.write_to_oam_dma(&buffer);
                // TODO 驱动多个 PPU 周期
            }
            0x4000..=0x4013 | 0x4015 => {
                println!("Ignoring APU Register access at {}", addr);
            }
            0x4016 => {
                self.joypad.write(data);
            }
            0x4017 => {
                println!("Ignoring joypad 2");
            }
            0x8000..=0xffff => { // PRG ROM
                panic!("Attempt to write to Cartridge ROM space")
            }
            _ => {
                println!("Ignoring mem access at {:04x}", addr);
            }
        }
    }
}