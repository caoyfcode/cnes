use bitflags::bitflags;

bitflags! {
    pub struct Button: u8 {
        const A = 0b0000_0001;
        const B = 0b0000_0010;
        const SECLECT = 0b0000_0100;
        const START = 0b0000_1000;
        const UP = 0b0001_0000;
        const DOWN = 0b0010_0000;
        const LEFT = 0b0100_0000;
        const RIGHT = 0b1000_0000;
    }
}

/// The controller operates in 2 modes:
/// - strobe bit on - controller reports only status of the button A on every read
/// - strobe bit off - controller cycles through all buttons
pub struct Joypad {
    strobe: bool,
    button_idx: u8,
    button: Button,
}

impl Joypad {
    pub fn new() -> Self {
        Joypad {
            strobe: false,
            button_idx: 0,
            button: Button::from_bits_truncate(0),
        }
    }

    pub fn write(&mut self, data: u8) {
        match data & 0x1 {
            0x00 => self.strobe = false,
            0x01 => {
                self.strobe = true;
                self.button_idx = 0;
            }
            _ => panic!("can't be here"),
        }
    }

    pub fn read(&mut self) -> u8 {
        let result = (self.button.bits >> self.button_idx) & 0x1;
        if !self.strobe {
            self.button_idx = (self.button_idx + 1) % 8;
        }
        result
    }

    pub fn set_button_pressed(&mut self, button: Button, pressed: bool) {
        self.button.set(button, pressed);
    }
}