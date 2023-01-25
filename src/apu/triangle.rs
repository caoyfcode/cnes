use super::length_counter::LengthCounter;


pub(super) struct Triangle {
    linear_counter: LinearCounter,
    length_counter: LengthCounter,
    timer_reset: u16, // 11bit timer, 用于控制频率
    timer_counter: u16,
    sequencer_step: usize, // 0..32
}

impl Triangle {
    const WAVE_TABLE: [u8; 32] = [ // 波形
        15, 14, 13, 12, 11, 10,  9, 8, 7, 6, 5, 4, 3, 2, 1, 0,
        0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15
    ];

    pub(super) fn new() -> Self {
        Self {
            linear_counter: LinearCounter::new(),
            length_counter: LengthCounter::new(),
            timer_reset: 0,
            timer_counter: 0,
            sequencer_step: 0,
        }
    }

    // $4008 CRRR.RRRR Linear counter setup (write)
    // $4008 Hlll.llll Triangle channel length counter halt and linear counter load (write)
    pub(super) fn write_linear_counter(&mut self, data: u8) {
        self.linear_counter.set_control_and_reload_value(data);
        self.length_counter.set_halt_flag(data & 0b1000_0000 == 0b1000_0000)
    }

    // $400A timer low 8 bits
    pub(super) fn write_timer_lo(&mut self, data: u8) {
        self.timer_reset = (self.timer_reset & 0xff00) | (data as u16);
    }

    // $400B  llll.lHHH  Length counter load and timer high (write)
    //
    // Side effects: Sets the linear counter reload flag
    pub(super) fn write_length_load_and_timer_hi(&mut self, data: u8) {
        self.timer_reset = (((data & 0b111) as u16) << 8) | (self.timer_reset & 0xff);
        self.timer_counter = self.timer_reset;
        self.linear_counter.set_reload_flag();
        self.length_counter.load_if_enabled_flag(data >> 3);
    }

    /// Status ($4015)
    pub(super) fn set_enabled_flag(&mut self, enabled: bool) {
        self.length_counter.set_enabled_flag(enabled);
    }

    /// Status ($4015) read
    pub(super) fn length_counter(&self) -> u8 {
        self.length_counter.counter()
    }

    pub(super) fn on_clock(&mut self) {
        // timer 滴答
        if self.timer_counter != 0 {
            self.timer_counter -= 1;
        } else {
            self.timer_counter = self.timer_reset;
            // 这里 sequencer 与 pulse 不太一样, 只有两个 counter 均非零才步进
            if self.length_counter.counter() != 0 && self.linear_counter.counter() != 0 {
                self.sequencer_step = (self.sequencer_step + 1) % 32;
            }
        }
    }

    pub(super) fn on_quarter_frame(&mut self) {
        self.linear_counter.on_quarter_frame();
    }

    pub(super) fn on_half_frame(&mut self) {
        self.length_counter.on_half_frame();
    }

    pub(super) fn output(&self) -> f32 {
        if self.timer_reset < 2 && self.length_counter() != 0 { // 超声波
            7.5f32
        } else {
            // 这里不判断两个 counter, 该通道因 counter 静音的原理是 counter 为 0, step 就不变了
            Self::WAVE_TABLE[self.sequencer_step] as f32
        }
    }
}

struct LinearCounter {
    control_flag: bool, // 置零可以保证隔一个 quarter frame 后不再 reload
    reload_flag: bool, // 控制 counter 是否 reload
    reload_val: u8, // 7 bit reload value
    counter: u8,
}

impl LinearCounter {

    fn new() -> Self {
        Self {
            reload_flag: true,
            control_flag: true,
            reload_val: 0,
            counter: 0,
        }
    }

    fn counter(&self) -> u8 {
        self.counter
    }

    // $4008  CRRR.RRRR  Linear counter setup (write)
    fn set_control_and_reload_value(&mut self, val: u8) {
        self.control_flag = val & 0b1000_0000 == 0b1000_0000;
        self.reload_val = val & 0b0111_1111;
    }

    // $400B 的副作用
    fn set_reload_flag(&mut self) {
        self.reload_flag = true
    }

    fn on_quarter_frame(&mut self) {
        if self.reload_flag {
            self.counter = self.reload_val;
        } else if self.counter != 0 {
            self.counter -= 1;
        }
        if !self.control_flag {
            self.reload_flag = false;
        }
    }
}