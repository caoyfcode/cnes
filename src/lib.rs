mod cpu;
mod bus;
mod cartridge;
mod ppu;
mod apu;
mod joypad;
mod common;

use std::{collections::HashMap, sync::Arc, mem::MaybeUninit, time::{Duration, Instant}};

use bus::Bus;
use cartridge::Rom;
use cpu::CPU;
use ringbuf::{Producer, HeapRb, Consumer, SharedRb};
use sdl2::{pixels::PixelFormatEnum, event::Event, keyboard::Keycode, audio::{AudioSpecDesired, AudioCallback}};


pub fn run(filename: &str) {
    env_logger::init();
    let sdl_ctx = sdl2::init().unwrap();
    let video_sys = sdl_ctx.video().unwrap();
    let audio_sys = sdl_ctx.audio().unwrap();

    // open a window
    let win = video_sys
        .window(filename, 256 * 3, 224 * 3)
        .position_centered()
        .build().unwrap();

    // open a playback
    let desired_spec = AudioSpecDesired {
        freq: Some(44100),
        channels: Some(1),  // mono
        samples: None       // default sample size
    };
    let buffer = HeapRb::<f32>::new(261 * 341 * 60 / 3 + 100);
    let (producer, consumer) = buffer.split();
    let mut sender = AudioSender::new(producer, (261 * 341 * 60 / 3) as f32, 44100f32);
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
    key_map.insert(Keycode::W, (joypad::Id::P1, joypad::Button::UP));
    key_map.insert(Keycode::A, (joypad::Id::P1, joypad::Button::LEFT));
    key_map.insert(Keycode::S, (joypad::Id::P1, joypad::Button::DOWN));
    key_map.insert(Keycode::D, (joypad::Id::P1, joypad::Button::RIGHT));
    key_map.insert(Keycode::RShift, (joypad::Id::P1, joypad::Button::SECLECT));
    key_map.insert(Keycode::Return, (joypad::Id::P1, joypad::Button::START));
    key_map.insert(Keycode::J, (joypad::Id::P1, joypad::Button::B));
    key_map.insert(Keycode::K, (joypad::Id::P1, joypad::Button::A));
    // P2
    key_map.insert(Keycode::Up, (joypad::Id::P2, joypad::Button::UP));
    key_map.insert(Keycode::Left, (joypad::Id::P2, joypad::Button::LEFT));
    key_map.insert(Keycode::Down, (joypad::Id::P2, joypad::Button::DOWN));
    key_map.insert(Keycode::Right, (joypad::Id::P2, joypad::Button::RIGHT));
    key_map.insert(Keycode::Kp8, (joypad::Id::P2, joypad::Button::SECLECT));
    key_map.insert(Keycode::Kp9, (joypad::Id::P2, joypad::Button::START));
    key_map.insert(Keycode::Kp2, (joypad::Id::P2, joypad::Button::B));
    key_map.insert(Keycode::Kp3, (joypad::Id::P2, joypad::Button::A));

    let rom = std::fs::read(filename).unwrap();
    let rom = Rom::new(&rom).unwrap();

    let mut frame_cnt = 0;
    let start = Instant::now();
    let bus = Bus::new_with_frame_callback(rom, move |ppu, joypad, samples| {
        // 开启垂直同步后, 帧率会有所限制(60Hz左右), 与NES CPU主频相符(1.8MHz*3/(341*262)=60.44Hz)
        log::info!("frame {} start", frame_cnt);

        texture.update(None, &ppu.frame().data[256 * 3 * 8..(256 * 3 * 232)], 256 * 3).unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();
        log::info!("get {} samples", samples.len());
        sender.append_samples(samples);

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
        let now = start.elapsed().as_secs_f32();
        let last = (frame_cnt + 1) as f32 / 60f32;
        if last > now {
            std::thread::sleep(Duration::from_secs_f32(last - now));
        }
        log::info!("frame {} end", frame_cnt);
        frame_cnt += 1;
    });

    let mut cpu = CPU::new(bus);
    cpu.reset();
    cpu.run();
}

type SamplesProducer = Producer<f32, Arc<SharedRb<f32, Vec<MaybeUninit<f32>>>>>;
type SamplesConsumer = Consumer<f32, Arc<SharedRb<f32, Vec<MaybeUninit<f32>>>>>;

struct AudioSender {
    producer: SamplesProducer,
    input_frequency: f32,
    output_frequency: f32,
    fraction: f32,
}

impl AudioSender {
    fn new(producer: SamplesProducer, input_frequency: f32, output_frequency: f32) -> Self {
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
    consumer: SamplesConsumer,
}

impl AudioReceiver {
    fn new(consumer: SamplesConsumer) -> Self {
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
