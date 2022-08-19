mod cpu;
mod bus;
mod cartridge;
mod ppu;

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

    let rom = std::fs::read(filename).unwrap();
    let rom = Rom::new(&rom).unwrap();

    let bus = Bus::new_with_frame_callback(rom, move |ppu| {
        texture.update(None, &ppu.frame().data, 256 * 3).unwrap();
        canvas.copy(&texture, None, None).unwrap();
        canvas.present();

        for event in event_pump.poll_iter() {
            match event {
                Event::Quit { .. } | Event::KeyDown { keycode: Some(Keycode::Escape), .. } => {
                    std::process::exit(0);
                }
                _ => {}
            }
        }
    });

    let mut cpu = CPU::new(bus);
    cpu.reset();
    cpu.run();
}

// fn show_tile(chr_rom: &Vec<u8>, bank: usize, tile_n: usize) -> Frame {
//     assert!(bank <= 1);

//     let mut frame = Frame::new();
//     let bank = (bank * 0x1000) as usize;
//     let tile_base = bank + tile_n * 16;
//     let tile = &chr_rom[tile_base..(tile_base + 16)];

//     for y in 0..8usize {
//         let lo = tile[y];
//         let hi = tile[y + 8];

//         for x in 0..8usize {
//             let hi = (hi >> (7 - x)) & 0x1;
//             let lo = (lo >> (7 - x)) & 0x1;
//             let color = ((hi) << 1) | lo;
//             let rgb = match color {
//                 0 => frame::SYSTEM_PALLETE[0x01],
//                 1 => frame::SYSTEM_PALLETE[0x23],
//                 2 => frame::SYSTEM_PALLETE[0x27],
//                 3 => frame::SYSTEM_PALLETE[0x30],
//                 _ => panic!("color can't be {:02x}", color),
//             };
//             frame.set_pixel(x, y, rgb);
//         }
//     }
//     frame
// }