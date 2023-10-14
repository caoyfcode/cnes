use std::{collections::HashMap, time::{Duration, Instant}};
use ringbuf::{HeapRb, HeapProducer, HeapConsumer};
use sdl2::{pixels::PixelFormatEnum, event::Event, keyboard::Keycode, audio::{AudioSpecDesired, AudioCallback}};
use crate::{Cpu, Rom, PlayerId, JoypadButton};

// 帧率应为 60 左右, 从 NES CPU主频的计算方式: 1.8MHz * 3 / (341*262) = 60.44Hz
const FPS: f32 = 60f32;
const FRAME_TIME: f32 = 1f32 / FPS;

pub fn run(rom_filename: &str) {
    env_logger::init();
    let sdl_ctx = sdl2::init().unwrap();
    let video_sys = sdl_ctx.video().unwrap();
    let audio_sys = sdl_ctx.audio().unwrap();

    // open a window
    let win = video_sys
        .window(rom_filename, 256 * 3, 224 * 3)
        .position_centered()
        .build().unwrap();

    // open a playback
    let desired_spec = AudioSpecDesired {
        freq: Some(44100),
        channels: Some(1),  // mono
        samples: None       // default sample size
    };
    let buffer = HeapRb::<f32>::new(262 * 341 * 60 / 3 + 100);
    let (producer, consumer) = buffer.split();
    let mut sender = AudioSender::new(producer, (262 * 341 * 60 / 3) as f32, 44100f32);
    let device = audio_sys.open_playback(
        None,
        &desired_spec,
        |_| AudioReceiver::new(consumer)
    ).unwrap();
    device.resume();

    let mut canvas = win.into_canvas().build().unwrap();
    let mut event_pump = sdl_ctx.event_pump().unwrap();
    canvas.set_scale(3.0, 3.0).unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator.create_texture_target(PixelFormatEnum::RGB24, 256, 224).unwrap();

    let mut key_map = HashMap::new();
    // P1
    key_map.insert(Keycode::W, (PlayerId::P1, JoypadButton::UP));
    key_map.insert(Keycode::A, (PlayerId::P1, JoypadButton::LEFT));
    key_map.insert(Keycode::S, (PlayerId::P1, JoypadButton::DOWN));
    key_map.insert(Keycode::D, (PlayerId::P1, JoypadButton::RIGHT));
    key_map.insert(Keycode::RShift, (PlayerId::P1, JoypadButton::SELECT));
    key_map.insert(Keycode::Return, (PlayerId::P1, JoypadButton::START));
    key_map.insert(Keycode::J, (PlayerId::P1, JoypadButton::B));
    key_map.insert(Keycode::K, (PlayerId::P1, JoypadButton::A));
    // P2
    key_map.insert(Keycode::Up, (PlayerId::P2, JoypadButton::UP));
    key_map.insert(Keycode::Left, (PlayerId::P2, JoypadButton::LEFT));
    key_map.insert(Keycode::Down, (PlayerId::P2, JoypadButton::DOWN));
    key_map.insert(Keycode::Right, (PlayerId::P2, JoypadButton::RIGHT));
    key_map.insert(Keycode::Kp8, (PlayerId::P2, JoypadButton::SELECT));
    key_map.insert(Keycode::Kp9, (PlayerId::P2, JoypadButton::START));
    key_map.insert(Keycode::Kp2, (PlayerId::P2, JoypadButton::B));
    key_map.insert(Keycode::Kp3, (PlayerId::P2, JoypadButton::A));

    let rom_bytes = std::fs::read(rom_filename).unwrap();
    let rom = Rom::new(&rom_bytes).unwrap();
    let mut cpu = Cpu::new(rom);
    cpu.reset();

    let mut frame_cnt = 0;
    // 用于帧率控制的时刻于帧数
    let mut base_instant = Instant::now();
    let mut base_frame = 0;

    loop {
        log::info!("Frame {} start", frame_cnt);
        
        // input
        let (_, joypad, _) = cpu.io_interface();
        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    std::process::exit(0);
                }
                Event::KeyDown {keycode: Some(key), .. } => {
                    if let Some((id, button)) = key_map.get(&key) {
                        joypad.set_button_pressed(*id, *button, true);
                    }
                }
                Event::KeyUp{keycode: Some(key), .. } => {
                    if let Some((id, button)) = key_map.get(&key) {
                        joypad.set_button_pressed(*id, *button, false);
                    }
                }
                _ => {}
            }
        }

        // update
        cpu.run_next_frame();
        let (frame, _, samples) = cpu.io_interface();
        sender.input_frequency = samples.data().len() as f32 * FPS;
        sender.append_samples(samples.data());
        samples.clear();

        // render
        texture.update(None, &frame.data()[256 * 3 * 8..(256 * 3 * 232)], 256 * 3).unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        // sleep
        let secs_from_base = base_instant.elapsed().as_secs_f32();
        let next_secs_from_base = (frame_cnt + 1 - base_frame) as f32 / FPS;
        if next_secs_from_base > secs_from_base {
            std::thread::sleep(Duration::from_secs_f32(next_secs_from_base - secs_from_base));
        } else if secs_from_base - next_secs_from_base > FRAME_TIME * 0.5 {
            base_frame = frame_cnt + 1;
            base_instant = Instant::now();
        }
        
        log::info!("Frame {} end", frame_cnt);
        frame_cnt += 1;
    }
}

struct AudioSender {
    producer: HeapProducer<f32>,
    input_frequency: f32,
    output_frequency: f32,
    fraction: f32,
}

impl AudioSender {
    fn new(producer: HeapProducer<f32>, input_frequency: f32, output_frequency: f32) -> Self {
        Self {
            producer,
            input_frequency,
            output_frequency,
            fraction: 0f32,
        }
    }

    fn append_samples(&mut self, samples: &[f32]) {
        let ratio = self.input_frequency / self.output_frequency;
        for sample in samples {
            while self.fraction <= 0f32 {
                if self.producer.push(*sample).is_err() { // 样本满了则等待声音线程播放一些
                   std::thread::sleep(std::time::Duration::from_micros(10));
                }
                self.fraction += ratio;
            }
            self.fraction -= 1f32;
        }
    }
}

struct AudioReceiver {
    consumer: HeapConsumer<f32>,
}

impl AudioReceiver {
    fn new(consumer: HeapConsumer<f32>) -> Self {
        Self {
            consumer
        }
    }
}

impl AudioCallback for AudioReceiver {
    type Channel = f32;

    fn callback(&mut self, out: &mut [f32]) {
        for x in out.iter_mut() {
            *x = match self.consumer.pop() {
                Some(sample) => sample,
                None => 0f32,
            }
        }
    }
}
