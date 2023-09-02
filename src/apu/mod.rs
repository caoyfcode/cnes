mod frame_counter;
// 通道
mod pulse;
mod triangle;
mod noise;
mod dmc;
// 通道需要的组件
mod envelope;
mod length_counter;

use crate::common::{Clock, Mem};

use self::{frame_counter::{FrameCounter, FrameCounterSignal}, pulse::Pulse, triangle::Triangle, noise::Noise, dmc::Dmc};

// 每个通道在每个 CPU 周期生成一个 sample (大约1.8MHz), 各个通道每周期生成 sample 要根据一系列组成部件的状态决定生成什么, 各通道需要用到的部件有:
// - **Frame Counter(帧计数器)** 用来驱动各通道的 Envelope, Sweep, Length Counter 和 Linear counter, 其每帧会生成 4 次 quarter frame 信号(2 次half frame), 可以工作在4步或5步模式下(step4, step5). 可以(optionally) 在 4 步模式的最后一步发出一次软中断(irq)
// - **Length Counter** 方波, 三角波, 噪声通道均有各自的 Length Counter. Length Counter 到达 0 后对应通道静音. 在 half frame 滴答
// - **Linear Counter** 三角波独有, 是更精确的 Length Counter, 亦是在达到 0 后静音. 在 quarter frame 滴答
// - **Envelope generator(包络发生器)** 方波与噪音通道包含, 用来生成包络, 在 quarter frame 滴答
// - **Sweep(扫描单元)** 只有方波通道有, 通过控制 Timer 来控制波形的频率变化, 在 half frame 滴答
// - **Sequencer(序列生成单元)** 方波与三角波通道有, 用来生成基础波形, 由 Timer 驱动
// - **Timer** 在所有通道中使用, 用来驱动 Sequencer 生成波形, 可以通过改变 Timer 来控制频率. 其包含一个由 CPU 周期驱动的分频器. 通过分频器, 三角波通道的 Timer 每一个 CPU 周期滴答一次, 其余所有通道每 2 个 CPU 周期滴答一次

pub(crate) struct Apu {
    // 通道
    pulse1: Pulse,
    pulse2: Pulse,
    triangle: Triangle,
    noise: Noise,
    dmc: Dmc,
    // 其他组成部分
    frame_counter: FrameCounter,
    // 状态信息
    samples: Samples,
}

/// audio samples
pub struct Samples {
    data: Vec<f32>
}

impl Samples {
    pub fn data(&self) -> &[f32] {
        &self.data
    }

    pub fn clear(&mut self) {
        self.data.clear()
    }
}

impl Apu {
    pub(crate) fn new() -> Self {
        Self {
            pulse1: Pulse::new(pulse::PulseId::Pulse1),
            pulse2: Pulse::new(pulse::PulseId::Pulse2),
            triangle: Triangle::new(),
            noise: Noise::new(),
            dmc: Dmc::new(),
            frame_counter: FrameCounter::new(),
            samples: Samples { data: Vec::new() },
        }
    }

    fn generate_a_sample(&mut self) {
        let pulse1 = self.pulse1.output() as f32;
        let pulse2 = self.pulse2.output() as f32;
        let pulse1_plus_pulse2 = pulse1 + pulse2;
        let pulse_out = if pulse1_plus_pulse2 == 0f32 {
            0f32
        } else {
            95.88 / (8128f32 / pulse1_plus_pulse2 + 100f32)
        };
        let triangle = self.triangle.output() as f32;
        let noise = self.noise.output() as f32;
        let dmc = self.dmc.output() as f32;
        let tnd_plus = triangle / 8227f32 + noise / 12241f32 + dmc / 22638f32;
        let tnd_out = if tnd_plus == 0f32 {
            0f32
        } else {
            159.79 / (1f32 / tnd_plus + 100f32)
        };
        self.samples.data.push(pulse_out + tnd_out);
    }

    pub(crate) fn mut_samples(&mut self) -> &mut Samples {
        &mut self.samples
    }

    pub(crate) fn irq(&self) -> bool {
        self.frame_counter.frame_interrupt() && self.dmc.interrupt()
    }

    /// DMC 是否需要加载 sample
    pub(crate) fn request_dma(&self) -> Option<u16> {
        self.dmc.request_dma()
    }

    // 当 request_dma 为 Some 时调用
    pub(crate) fn load_dma_data(&mut self, data: u8) {
        self.dmc.load_sample(data);
    }

    // $4015 read | IF-D NT21 | DMC interrupt (I), frame interrupt (F), DMC active (D), length counter > 0 (N/T/2/1)
    fn read_status(&mut self) -> u8 {
        let mut status = 0u8;
        if self.dmc.interrupt() {
            status |= 0b1000_0000;
        }
        if self.frame_counter.poll_frame_interrupt() {
            status |= 0b0100_0000;
        }
        if self.dmc.bytes_remaining() > 0 {
            status |= 0b0001_0000;
        }
        if self.noise.length_counter() > 0 {
            status |= 0b1000;
        }
        if self.triangle.length_counter() > 0 {
            status |= 0b0100;
        }
        if self.pulse2.length_counter() > 0 {
            status |= 0b0010;
        }
        if self.pulse1.length_counter() > 0 {
            status |= 0b0001;
        }
        status
    }

    // $4015 write | ---D NT21 | Enable DMC (D), noise (N), triangle (T), and pulse channels (2/1)
    fn write_status(&mut self, data: u8) {
        self.dmc.set_enabled(data & 0b0001_0000 == 0b0001_0000);
        self.noise.set_enabled_flag(data & 0b1000 == 0b1000);
        self.triangle.set_enabled_flag(data & 0b0100 == 0b0100);
        self.pulse2.set_enabled_flag(data & 0b0010 == 0b0010);
        self.pulse1.set_enabled_flag(data & 0b0001 == 0b0001);
    }

}

impl Clock for Apu {
    type Result = ();

    fn clock(&mut self) -> Self::Result {
        let FrameCounterSignal {
            quarter_frame,
            half_frame,
            apu_clock,
        } = self.frame_counter.clock();
        if quarter_frame {
            self.pulse1.on_quarter_frame();
            self.pulse2.on_quarter_frame();
            self.triangle.on_quarter_frame();
            self.noise.on_quarter_frame();
        }
        if half_frame {
            self.pulse1.on_half_frame();
            self.pulse2.on_half_frame();
            self.triangle.on_half_frame();
            self.noise.on_half_frame();
        }
        if apu_clock {
            self.pulse1.on_apu_clock();
            self.pulse2.on_apu_clock();
            self.noise.on_apu_clock();
            self.dmc.on_apu_clock();
        }
        self.triangle.on_clock();

        self.generate_a_sample();
    }
}

impl Mem for Apu {
    fn mem_read(&mut self, addr: u16) -> u8 {
        if addr == 0x4015 {
            self.read_status()
        } else {
            log::warn!("Attempt to read from write-only APU Register address {:04x}", addr);
            0
        }
    }

    fn mem_write(&mut self, addr: u16, data: u8) {
        match addr {
            // pulse 1
            0x4000 => self.pulse1.write_ctrl(data),
            0x4001 => self.pulse1.write_sweep(data),
            0x4002 => self.pulse1.write_timer_lo(data),
            0x4003 => self.pulse1.write_length_load_and_timer_hi(data),
            // pulse 2
            0x4004 => self.pulse2.write_ctrl(data),
            0x4005 => self.pulse2.write_sweep(data),
            0x4006 => self.pulse2.write_timer_lo(data),
            0x4007 => self.pulse2.write_length_load_and_timer_hi(data),
            // triangle
            0x4008 => self.triangle.write_linear_counter(data),
            0x400a => self.triangle.write_timer_lo(data),
            0x400b => self.triangle.write_length_load_and_timer_hi(data),
            0x4009 => log::warn!("Attempt to write to unused APU Register address {:04x}", addr),
            // noise
            0x400c => self.noise.write_ctrl(data),
            0x400e => self.noise.write_mode_and_period(data),
            0x400f => self.noise.write_length_counter_load(data),
            0x400d => log::warn!("Attempt to write to unused APU Register address {:04x}", addr),
            // DMC
            0x4010 => self.dmc.write_flags_and_rate(data),
            0x4011 => self.dmc.write_direct_load(data),
            0x4012 => self.dmc.write_sample_address(data),
            0x4013 => self.dmc.write_sample_length(data),
            // status
            0x4015 => self.write_status(data),
            // frame counter
            0x4017 => self.frame_counter.write(data),
            _ => (),
        }
    }
}