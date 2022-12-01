use crate::common::Clock;

enum Mode {
    Step4, // 4 步模式
    Step5, // 5 步模式
}

pub(super) struct FrameCounterSignal {
    quarter_frame: bool, // 约 1/4 帧
    half_frame: bool, // 约 1/2 帧一次
    apu_clock: bool, // 每 2 个 cpu clock 一次
}



/// FrameCounter
/// 定期产生特定信号用于驱动其他 APU 组件:
/// - APU clock: 每 2 个 CPU 周期一次
/// - quarter frame: 约每 1/4 帧一次
/// - half frame: 约每 1/2 帧一次
///
/// 在不同工作模式下每 4 个 quarter frame 中的最后一个有所不同(与之同时的 half frame 亦然),
/// 且 4 步模式下第四个 quarter frame 将可能产生软中断
pub(super) struct FrameCounter {
    // 组成
    mode: Mode, // 工作模式
    frame_interrupt_flag: bool, // 是否产生了软中断
    interrupt_inhibit_flag: bool, // 是否屏蔽中断
    // 状态信息
    step: usize, // 0..=5
    cycles: u32, // 计数器
    write_val: Option<u8>, // 延迟的写数值
    write_delay: u8, // 延迟的 CPU 周期数
}

impl FrameCounter {
    /// 使用 NTSC 标准, 见 https://www.nesdev.org/wiki/APU_Frame_Counter,
    /// 但是使用 CPU 周期数计数而非 APU 周期数
    const STEP_CYCLES: [[u32; 6]; 2] = [
        [7457, 14913, 22371, 29828, 29829, 29830], // Step4
        [7457, 14913, 22371, 29829, 37281, 37282]  // Step5
    ];

    pub(super) const fn new() -> Self {
        Self {
            mode: Mode::Step4,
            frame_interrupt_flag: false,
            interrupt_inhibit_flag: false,
            step: 0,
            cycles: 0,
            write_val: None,
            write_delay: 0,
        }
    }

    // If the write occurs during an APU cycle, the effects occur 3 CPU cycles after the $4017 write cycle, and if the write occurs between APU cycles, the effects occurs 4 CPU cycles after the write cycle.
    pub(super) fn write(&mut self, data: u8) {
        self.write_val = Some(data);
        self.write_delay = if self.apu_clock() {
            4
        } else {
            3
        };
    }


    /// 是否在 APU 周期的边缘, 即偶数 CPU 周期
    fn apu_clock(&self) -> bool {
        self.cycles & 0x01 == 0
    }


    // $4017 | MI-- ---- | Mode (M, 0 = 4-step, 1 = 5-step), IRQ inhibit flag (I)
    fn handle_delayed_write_per_cycle(&mut self) {
        if self.write_val.is_none() {
            return;
        }
        if self.write_delay != 0 {
            self.write_delay -= 1;
            return;
        }
        let val = self.write_val.take().unwrap();
        if val & 0x80 == 0x80 {
            self.mode = Mode::Step5;
        } else {
            self.mode = Mode::Step4;
        }
        self.interrupt_inhibit_flag = val & 0x40 == 0x40;

        self.step = 0;
        self.cycles = 0;
        self.frame_interrupt_flag = false;
    }

    // 取出 frame interrupt 并置 0
    pub(super) fn poll_frame_interrupt(&mut self) -> bool {
        let ret = self.frame_interrupt_flag;
        self.frame_interrupt_flag = false;
        ret
    }

}

impl Clock for FrameCounter {
    type Result = FrameCounterSignal;
    fn clock(&mut self) -> Self::Result {
        self.handle_delayed_write_per_cycle();
        let apu_clock = self.apu_clock();
        let (mode_idx, should_interrupt) = match self.mode {
            Mode::Step4 => (0, !self.interrupt_inhibit_flag),
            Mode::Step5 => (1, false),
        };
        let mut quarter_frame = false;
        let mut half_frame = false;

        if self.cycles == FrameCounter::STEP_CYCLES[mode_idx][self.step] {
            match self.step {
                0 | 2 => {
                    quarter_frame = true;
                }
                1 => {
                    quarter_frame = true;
                    half_frame = true;
                }
                4 => {
                    quarter_frame = true;
                    half_frame = true;
                    self.frame_interrupt_flag = should_interrupt;
                }
                3 => {
                    self.frame_interrupt_flag = should_interrupt;
                }
                5 => {
                    self.frame_interrupt_flag = should_interrupt;
                    self.cycles = 0; // 最后一个周期等同于周期 0, 下一个就是 1
                }
               _ => panic!("can't be here"),
            }
            self.step = (self.step + 1) % 6;
        }

        self.cycles += 1;

        FrameCounterSignal {
            quarter_frame,
            half_frame,
            apu_clock,
        }
    }

}