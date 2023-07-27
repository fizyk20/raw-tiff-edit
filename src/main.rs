use std::{
    fs::File,
    io::{Read, Write},
};

mod rawloader;

use image::{ImageBuffer, Luma, Pixel};
use imageproc::drawing::draw_text_mut;
use rawloader::*;
use rusttype::{FontCollection, Scale};

static FONT: &[u8] = include_bytes!("DejaVuSans.ttf");

fn draw_text(img: &mut ImageBuffer<Luma<u16>, Vec<<Luma<u16> as Pixel>::Subpixel>>) {
    let font = FontCollection::from_bytes(FONT)
        .unwrap()
        .into_font()
        .unwrap();
    let scale = Scale { x: 400.0, y: 400.0 };
    draw_text_mut(
        img,
        Luma([17216]),
        1000,
        1800,
        scale,
        &font,
        &format!("EDITED BY SIO"),
    );
}

fn main() {
    let mut file = File::open("Y-DP-105mm-9480.ARW").unwrap();
    let mut buffer = vec![];
    file.read_to_end(&mut buffer).unwrap();

    let width: usize = 6048;
    let height: usize = 4024;
    let start = 839680;

    let mut decoded = decode_arw2(&buffer[start..], width, height);

    let mut img: ImageBuffer<Luma<u16>, Vec<<Luma<u16> as Pixel>::Subpixel>> =
        ImageBuffer::new(width as u32, height as u32);

    for y in 0..height {
        for x in 0..width {
            let val = decoded[y * width + x];
            img.put_pixel(x as u32, y as u32, Luma([val]));
        }
    }

    draw_text(&mut img);

    for y in 0..height {
        for x in 0..width {
            let pixel = img.get_pixel(x as u32, y as u32);
            decoded[y * width + x] = pixel.0[0];
        }
    }

    for (i, byte) in encode_arw2(&decoded, width).into_iter().enumerate() {
        buffer[start + i] = byte;
    }

    let mut file = File::create("edited.arw").unwrap();
    file.write(&buffer[..]).unwrap();
}
