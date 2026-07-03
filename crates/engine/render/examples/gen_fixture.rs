use image::codecs::gif::{GifEncoder, Repeat};
use image::{Frame, ImageBuffer, Rgba};
use std::fs::File;

fn main() {
    let (w, h) = (32u32, 32u32);

    // ── sprite.png: single opaque red disk on transparent ground ─────────────
    let mut img = ImageBuffer::from_pixel(w, h, Rgba([0u8, 0, 0, 0])); // transparent
    let (cx, cy, r) = (15.5f32, 15.5f32, 12.0f32);
    for y in 0..h {
        for x in 0..w {
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            if dx * dx + dy * dy <= r * r {
                img.put_pixel(x, y, Rgba([220u8, 70, 70, 255])); // opaque red disk
            }
        }
    }
    img.save("crates/render/tests/fixtures/sprite.png").unwrap();
    println!("Generated crates/render/tests/fixtures/sprite.png (32x32 RGBA, red disk on transparent ground)");

    // ── anim.gif: 4-frame animation, disk moves left→right and changes color ─
    //
    // Fixture spec (constants downstream tasks may rely on):
    //   dimensions: 32×32 RGBA8
    //   frame_count: 4
    //   background: (0,0,0,0) fully transparent
    //   disk: radius 6, center y = 16
    //     frame 0: cx=8,  color (220, 70,  70,  255) red
    //     frame 1: cx=14, color (70,  200, 70,  255) green
    //     frame 2: cx=20, color (70,  70,  220, 255) blue
    //     frame 3: cx=26, color (220, 200, 70,  255) yellow
    let frames: [(u32, [u8; 4]); 4] = [
        (8,  [220, 70,  70,  255]),
        (14, [70,  200, 70,  255]),
        (20, [70,  70,  220, 255]),
        (26, [220, 200, 70,  255]),
    ];

    let gif_path = "crates/render/tests/fixtures/anim.gif";
    let gif_file = File::create(gif_path).unwrap();
    {
        let mut encoder = GifEncoder::new_with_speed(gif_file, 10);
        encoder.set_repeat(Repeat::Infinite).unwrap();

        let r: i32 = 6;
        let cy: i32 = 16;

        for (cx, color) in frames {
            let mut frame_img: ImageBuffer<Rgba<u8>, Vec<u8>> =
                ImageBuffer::from_pixel(w, h, Rgba([0u8, 0, 0, 0])); // transparent background
            let cx = cx as i32;
            for y in 0..h {
                for x in 0..w {
                    let dx = x as i32 - cx;
                    let dy = y as i32 - cy;
                    if dx * dx + dy * dy <= r * r {
                        frame_img.put_pixel(x, y, Rgba(color));
                    }
                }
            }
            encoder.encode_frame(Frame::new(frame_img)).unwrap();
        }
    } // encoder drops + flushes here

    println!("Generated {gif_path} (32x32 RGBA, 4-frame animated disk, transparent background)");
}
