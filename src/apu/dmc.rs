pub(super) struct Dmc {
    interrupt_flag: bool,
    interrupt_enabled_flag: bool,
    // timer
    timer_reset: u16, // 共有 16 中不同的值(RATE_TABLE内)
    timer_counter: u16,
    // memory reader
    sample_buffer: u8, // 8-bit sample buffer
    sample_buffer_empty: bool,
    sample_address: u16, // sample 在内存中的地址
    sample_length: u16, // sample 数量(以 byte 计)
    current_address: u16, // 下一个 sample byte 在内存中的地址
    bytes_remaining: u16, // 除了 sample buffer 外剩余 sample 数(以 byte 计)
    loop_flag: bool, // 是否循环播放
    // output unit
    shift_register: u8, // right shift register
    bits_remaining: u8, // bits remaining counter
    output: u8, // 7-bit output level
    silence_flag: bool,
}

impl Dmc {
    /// 用来设置 timer_reset 达到 16 种不同的音符
    const RATE_TABLE: [u16; 16] = [
        0x1ac, 0x17c, 0x154, 0x140, 0x11e, 0xfe, 0xe2, 0xd6,
        0xbe, 0xa0, 0x8e, 0x80, 0x6a, 0x54, 0x48, 0x36
    ];

    pub(super) fn new() -> Self {
        Self {
            interrupt_flag: false,
            interrupt_enabled_flag: false,
            timer_reset: 0,
            timer_counter: 0,
            sample_buffer: 0,
            sample_buffer_empty: true,
            sample_address: 0,
            sample_length: 0,
            current_address: 0,
            bytes_remaining: 0,
            loop_flag: false,
            shift_register: 0,
            bits_remaining: 0,
            output: 0,
            silence_flag: true,
        }
    }

    fn start_sample(&mut self) {
        self.current_address = self.sample_address;
        self.bytes_remaining = self.sample_length;
    }

    pub(super) fn interrupt(&self) -> bool {
        self.interrupt_flag
    }

    pub(super) fn request_dma(&self) -> Option<u16> {
        if self.sample_buffer_empty && self.bytes_remaining != 0 {
            Some(self.current_address)
        } else {
            None
        }
    }

    /// 加载 request_dma 返回的地址处的值至 sample buffer
    ///
    /// 必须在 request_dma 返回 Some 时调用
    pub(super) fn load_sample(&mut self, data: u8) {
        self.sample_buffer = data;
        self.sample_buffer_empty = false;
        self.current_address = self.current_address.checked_add(1).unwrap_or(0x8000);
        self.bytes_remaining -= 1;
        if self.bytes_remaining == 0 {
            if self.loop_flag {
                self.start_sample();
            }
            if self.interrupt_enabled_flag {
                self.interrupt_flag = true;
            }
        }
    }

    /// $4010 IL--.RRRR Flags and Rate (write)
    /// - bit 7 I---.---- IRQ enabled flag. If clear, the interrupt flag is cleared.
    /// - bit 6 -L--.---- Loop flag
    /// - RRRR Rate index
    pub(super) fn write_flags_and_rate(&mut self, data: u8) {
        self.interrupt_enabled_flag = data & 0b1000_0000 == 0b1000_0000;
        if !self.interrupt_enabled_flag {
            self.interrupt_flag = false;
        }
        self.loop_flag = data & 0b0100_0000 == 0b0100_0000;
        let index = (data & 0b1111) as usize;
        self.timer_reset = Self::RATE_TABLE[index];
    }

    /// $4011 -DDD.DDDD Direct load (write)
    /// - bits 6-0 -DDD.DDDD The DMC output level is set to D, an unsigned value.
    pub(super) fn write_direct_load(&mut self, data: u8) {
        self.output = data & 0b0111_1111;
    }

    /// $4012 AAAA.AAAA Sample address (write)
    /// - Sample address = %11AAAAAA.AA000000 = $C000 + (A * 64)
    pub(super) fn write_sample_address(&mut self, data: u8) {
        self.sample_address = 0b1100_0000 | ((data as u16) << 6);
    }

    /// $4013 LLLL.LLLL Sample length (write)
    /// - bits 7-0 LLLL.LLLL Sample length = %LLLL.LLLL0001 = (L * 16) + 1 bytes
    pub(super) fn write_sample_length(&mut self, data: u8) {
        self.sample_length = (data as u16) << 4 + 1;
    }

    // $4015 write
    pub(super) fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.bytes_remaining = 0;
        } else {
            if self.bytes_remaining == 0 {
                self.start_sample();
            }
        }
        self.interrupt_flag = false;
    }

    /// $4015 read usage
    pub(super) fn bytes_remaining(&self) -> u16 {
        self.bytes_remaining
    }

    pub(super) fn on_apu_clock(&mut self) {
        // timer 滴答
        if self.timer_counter >= 2 {
            self.timer_counter -= 2; // 减 2 是因为 RATE_TABLE 储存的是 cpu clock 数
        } else {
            self.timer_counter = self.timer_reset;
            // https://www.nesdev.org/wiki/APU_DMC#Output_unit
            if !self.silence_flag {
                if self.shift_register & 0x1 == 0x1 {
                    if self.output <= 125 {
                        self.output += 2;
                    }
                } else {
                    if self.output >= 2 {
                        self.output -= 2;
                    }
                }
            }
            self.shift_register >>= 1;
            self.bits_remaining = self.bits_remaining.checked_sub(1).unwrap_or(0);
            if self.bits_remaining == 0 { // an output cycle ends
                self.bits_remaining = 8;
                if self.sample_buffer_empty {
                    self.silence_flag = true;
                } else {
                    self.silence_flag = false;
                    self.shift_register = self.sample_buffer;
                    self.sample_buffer_empty = true;
                }
            }
        }
    }

    pub(super) fn output(&self) -> u8 {
        self.output
    }
}