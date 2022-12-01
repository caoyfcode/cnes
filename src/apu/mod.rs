mod frame_counter;
mod envelope;

use self::frame_counter::FrameCounter;

// 每个通道在每个 CPU 周期生成一个 sample (大约1.8MHz), 各个通道每周期生成 sample 要根据一系列组成部件的状态决定生成什么, 各通道需要用到的部件有:
// - **Frame Counter(帧计数器)** 用来驱动各通道的 Envelope, Sweep, Length Counter 和 Linear counter, 其每帧会生成 4 次 quarter frame 信号(2 次half frame), 可以工作在4步或5步模式下(step4, step5). 可以(optionally) 在 4 步模式的最后一步发出一次软中断(irq)
// - **Length Counter** 方波, 三角波, 噪声通道均有各自的 Length Counter. Length Counter 到达 0 后对应通道静音. 在 half frame 滴答
// - **Linear Counter** 三角波独有, 是更精确的 Length Counter, 亦是在达到 0 后静音. 在 quarter frame 滴答
// - **Envelope generator(包络发生器)** 方波与噪音通道包含, 用来生成包络, 在 quarter frame 滴答
// - **Sweep(扫描单元)** 只有方波通道有, 通过控制 Timer 来控制波形的频率变化, 在 half frame 滴答
// - **Sequencer(序列生成单元)** 方波与三角波通道有, 用来生成基础波形, 由 Timer 驱动
// - **Timer** 在所有通道中使用, 用来驱动 Sequencer 生成波形, 可以通过改变 Timer 来控制频率. 其包含一个由 CPU 周期驱动的分频器. 通过分频器, 三角波通道的 Timer 每一个 CPU 周期滴答一次, 其余所有通道每 2 个 CPU 周期滴答一次

struct APU {
    frame_counter: FrameCounter,
}

impl APU {

}