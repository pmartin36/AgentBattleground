use image::{ImageBuffer, Rgba};

fn main() {
    let (w, h) = (32u32, 32u32);
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
}
