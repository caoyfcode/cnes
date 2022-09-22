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

#[derive(Clone, Copy)]
pub enum Id {
    P1,
    P2,
}

/// The controller operates in 2 modes:
/// - strobe bit on - controller reports only status of the button A on every read
/// - strobe bit off - controller cycles through all buttons
pub struct Joypad {
    strobe: bool,
    button_idx_p1: u8,
    button_idx_p2: u8,
    button_p1: Button,
    button_p2: Button,
}

impl Joypad {
    pub fn new() -> Self {
        Joypad {
            strobe: false,
            button_idx_p1: 0,
            button_idx_p2: 0,
            button_p1: Button::from_bits_truncate(0),
            button_p2: Button::from_bits_truncate(0),
        }
    }

    pub fn write(&mut self, data: u8) {
        match data & 0x1 {
            0x00 => self.strobe = false,
            0x01 => {
                self.strobe = true;
                self.button_idx_p1 = 0;
                self.button_idx_p2 = 0;
            }
            _ => panic!("can't be here"),
        }
    }

    pub fn read(&mut self, id: Id) -> u8 {
        let (button_idx, button) = match id {
            Id::P1 => (&mut self.button_idx_p1, &mut self.button_p1),
            Id::P2 => (&mut self.button_idx_p2, &mut self.button_p2),
        };
        let result = (button.bits >> *button_idx) & 0x1;
        if !self.strobe {
            *button_idx = (*button_idx + 1) % 8;
        }
        result
    }

    pub fn set_button_pressed(&mut self, id: Id, button: Button, pressed: bool) {
        match id {
            Id::P1 => self.button_p1.set(button, pressed),
            Id::P2 => self.button_p2.set(button, pressed),
        }
    }
}