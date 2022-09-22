mod cpu;
mod bus;
mod cartridge;
mod ppu;
mod joypad;
mod common;

use std::collections::HashMap;

use bus::Bus;
use cartridge::Rom;
use cpu::CPU;
use sdl2::{pixels::PixelFormatEnum, event::Event, keyboard::Keycode};

#[macro_use]
extern crate lazy_static;

pub fn run(filename: &str) {
    let sdl_ctx = sdl2::init().unwrap();
    let video_sys = sdl_ctx.video().unwrap();
    let win = video_sys
        .window(filename, 256 * 3, 240 * 3)
        .position_centered()
        .build().unwrap();

    let mut canvas = win.into_canvas().present_vsync().build().unwrap();
    let mut event_pump = sdl_ctx.event_pump().unwrap();
    canvas.set_scale(3.0, 3.0).unwrap();

    let creator = canvas.texture_creator();
    let mut texture = creator.create_texture_target(PixelFormatEnum::RGB24, 256, 240).unwrap();

    let mut key_map = HashMap::new();
    key_map.insert(Keycode::W, joypad::Button::UP);
    key_map.insert(Keycode::A, joypad::Button::LEFT);
    key_map.insert(Keycode::S, joypad::Button::DOWN);
    key_map.insert(Keycode::D, joypad::Button::RIGHT);
    key_map.insert(Keycode::RShift, joypad::Button::SECLECT);
    key_map.insert(Keycode::Return, joypad::Button::START);
    key_map.insert(Keycode::J, joypad::Button::B);
    key_map.insert(Keycode::K, joypad::Button::A);

    let rom = std::fs::read(filename).unwrap();
    let rom = Rom::new(&rom).unwrap();

    let bus = Bus::new_with_frame_callback(rom, move |ppu, joypad| {
        texture.update(None, &ppu.frame().data, 256 * 3).unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    std::process::exit(0); // 开启垂直同步后, 帧率会有所限制(60Hz左右), 与NES CPU主频相符(1.8MHz*3/(341*262)=60.44Hz)
                }
                Event::KeyDown {keycode: Some(key), .. } => {
                    if let Some(button) = key_map.get(&key) {
                        joypad.set_button_pressed(*button, true);
                    }
                }
                Event::KeyUp{keycode: Some(key), .. } => {
                    if let Some(button) = key_map.get(&key) {
                        joypad.set_button_pressed(*button, false);
                    }
                }
                _ => {}
            }
        }
    });

    let mut cpu = CPU::new(bus);
    cpu.reset();
    cpu.run();
}
