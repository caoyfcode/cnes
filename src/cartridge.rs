const NES_TAG: [u8; 4] = [0x4E, 0x45, 0x53, 0x1A]; // NES^Z
const PRG_ROM_PAGE_SIZE: usize = 16 * 1024; // INES 格式中 PRG ROM 为若干个 16KB
const CHR_ROM_PAGE_SIZE: usize = 8 * 1024; // INES 格式中 CHR ROM 为若干个 8 KB

/// PPU Mirroring type
/// - Horizontal
/// - Vertical
/// - 4 Screen
#[derive(Debug, PartialEq)]
#[allow(non_camel_case_types)]
pub enum Mirroring {
    VERTICAL,
    HORIZONTAL,
    FOUR_SCREEN,
}

pub struct Rom {
    pub prg_rom: Vec<u8>, // Program ROM
    pub chr_rom: Vec<u8>, // Character ROM
    pub mapper: u8,
    pub screen_mirroring: Mirroring,
}

impl Rom {
    /// 从 iNES 格式生成 rom
    /// + 文件头
    ///   - 0, 1, 2, 3: "NES^Z"
    ///   - 4: 16KB PRG-ROM Bank 的数目
    ///   - 5: 8KB CHR-ROM/VROM Bank的数目
    ///   - 6: 控制字节 1
    ///     * 0: 1 for vertical mirroring, 0 for horizontal
    ///     * 1: 1 for battery-backend RAM at $6000-$7fff
    ///     * 2: 1 for 512 byte trainer at $7000-$71ff
    ///     * 3: 1 for four-screen VRAM layout
    ///     * 7, 6, 5, 4: mapper type 低四字节
    ///   - 7: 控制字节 2
    ///     * 0: should be 0, for iNES 1.0
    ///     * 1: should be 0, for iNES 1.0
    ///     * 3, 2: 10 for iNES 2.0, 00 for iNES 1.0
    ///     * 7, 6, 5, 4: mapper type 高四字节
    ///   - 8: 8KB RAM Bank的数目, 为了与以前的iNES格式兼容, 为0时表示RAM的第1页
    ///   - 9, 10, 11, 12, 13, 14, 15: 0
    /// + (控制字节绝对是否存在)512 字节 trainer
    /// + PRG ROM
    /// + CHR ROM
    fn new(raw: &Vec<u8>) ->Result<Rom, String> {
        // 16 字节 NES header
        if &raw[0..4] != NES_TAG { // 4 字节: "NES^Z"
            return Err("File is not in iNES file format".to_string());
        }
        let prg_rom_size = raw[4] as usize * PRG_ROM_PAGE_SIZE;
        let chr_rom_size = raw[5] as usize * CHR_ROM_PAGE_SIZE;
        let (control1, control2) = (raw[6], raw[7]);
        let mapper = (control2 & 0b1111_0000) | (control1 >> 4);
        let vertical_mirroring = control1 & 1 == 1;
        let _sram = control1 & 0b10 == 0b10;
        let trainer = control1 & 0b100 == 0b100;
        let four_screen = control1 & 0b1000 == 0b1000;
        let screen_mirroring = match (vertical_mirroring, four_screen) {
            (_, true) => Mirroring::FOUR_SCREEN,
            (true, false) => Mirroring::VERTICAL,
            (false, false) => Mirroring::HORIZONTAL,
        };
        if control2 & 0b1111 != 0 {
            return Err("NES2.0 format is not supported".to_string());
        }
        let prg_rom_start = 16 + if trainer {512} else {0};
        let chr_rom_start = prg_rom_start + prg_rom_size;
        Ok(Rom {
            prg_rom: raw[prg_rom_start..(chr_rom_start)].to_vec(),
            chr_rom: raw[chr_rom_start..(chr_rom_start + chr_rom_size)].to_vec(),
            mapper,
            screen_mirroring,
        })
    }
}

#[cfg(test)]
pub mod tests {

    use super::*;

    struct TestRom {
        header: Vec<u8>,
        trainer: Option<Vec<u8>>,
        prg_rom: Vec<u8>,
        chr_rom: Vec<u8>,
    }

    fn create_rom(rom: TestRom) -> Vec<u8> {
        let mut result = Vec::with_capacity(
            rom.header.len()
                + rom.trainer.as_ref().map_or(0, |t| t.len())
                + rom.prg_rom.len()
                + rom.chr_rom.len(),
        );

        result.extend(&rom.header);
        if let Some(t) = rom.trainer {
            result.extend(t);
        }
        result.extend(&rom.prg_rom);
        result.extend(&rom.chr_rom);

        result
    }

    pub fn test_rom() -> Rom {
        let test_rom = create_rom(TestRom {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x02, 0x01, 0x31, 00, 00, 00, 00, 00, 00, 00, 00, 00,
            ],
            trainer: None,
            prg_rom: vec![1; 2 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        });

        Rom::new(&test_rom).unwrap()
    }

    pub fn test_rom_with_2_bank_prg(prg: Vec<u8>) -> Rom {
        if prg.len() > 2 * PRG_ROM_PAGE_SIZE {
            panic!("PRG bigger than 2 bank")
        }
        let mut test_rom = TestRom {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x02, 0x01, 0x31, 00, 00, 00, 00, 00, 00, 00, 00, 00,
            ],
            trainer: None,
            prg_rom: vec![1; 2 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        };
        test_rom.prg_rom[0..prg.len()].copy_from_slice(&prg);
        test_rom.prg_rom[0xfffc - 0x8000] = 0x00; // 程序起始地址
        test_rom.prg_rom[0xfffc - 0x8000 + 1] = 0x80;
        let test_rom = create_rom(test_rom);

        Rom::new(&test_rom).unwrap()
    }

    #[test]
    fn test() {
        let test_rom = create_rom(TestRom {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x02, 0x01, 0x31, 00, 00, 00, 00, 00, 00, 00, 00, 00,
            ],
            trainer: None,
            prg_rom: vec![1; 2 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        });

        let rom: Rom = Rom::new(&test_rom).unwrap();

        assert_eq!(rom.chr_rom, vec!(2; 1 * CHR_ROM_PAGE_SIZE));
        assert_eq!(rom.prg_rom, vec!(1; 2 * PRG_ROM_PAGE_SIZE));
        assert_eq!(rom.mapper, 3);
        assert_eq!(rom.screen_mirroring, Mirroring::VERTICAL);
    }

    #[test]
    fn test_with_trainer() {
        let test_rom = create_rom(TestRom {
            header: vec![
                0x4E,
                0x45,
                0x53,
                0x1A,
                0x02,
                0x01,
                0x31 | 0b100,
                00,
                00,
                00,
                00,
                00,
                00,
                00,
                00,
                00,
            ],
            trainer: Some(vec![0; 512]),
            prg_rom: vec![1; 2 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        });

        let rom: Rom = Rom::new(&test_rom).unwrap();

        assert_eq!(rom.chr_rom, vec!(2; 1 * CHR_ROM_PAGE_SIZE));
        assert_eq!(rom.prg_rom, vec!(1; 2 * PRG_ROM_PAGE_SIZE));
        assert_eq!(rom.mapper, 3);
        assert_eq!(rom.screen_mirroring, Mirroring::VERTICAL);
    }

    #[test]
    fn test_nes2_is_not_supported() {
        let test_rom = create_rom(TestRom {
            header: vec![
                0x4E, 0x45, 0x53, 0x1A, 0x01, 0x01, 0x31, 0x8, 00, 00, 00, 00, 00, 00, 00, 00,
            ],
            trainer: None,
            prg_rom: vec![1; 1 * PRG_ROM_PAGE_SIZE],
            chr_rom: vec![2; 1 * CHR_ROM_PAGE_SIZE],
        });
        let rom = Rom::new(&test_rom);
        match rom {
            Result::Ok(_) => assert!(false, "should not load rom"),
            Result::Err(str) => assert_eq!(str, "NES2.0 format is not supported"),
        }
    }
}