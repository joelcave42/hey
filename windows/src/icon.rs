//! Hey's icon, drawn in code so there are no image files to ship: a white
//! speech bubble with three dots on an indigo-to-blue rounded square.

pub fn rgba(size: u32) -> Vec<u8> {
    const SAMPLES: u32 = 4;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let (mut r, mut g, mut b, mut a) = (0.0, 0.0, 0.0, 0.0);
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let u = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    let v = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) / size as f32;
                    if let Some([pr, pg, pb]) = sample(u, v) {
                        r += pr;
                        g += pg;
                        b += pb;
                        a += 1.0;
                    }
                }
            }
            let n = (SAMPLES * SAMPLES) as f32;
            if a > 0.0 {
                out.extend([
                    (r / a) as u8,
                    (g / a) as u8,
                    (b / a) as u8,
                    (a / n * 255.0) as u8,
                ]);
            } else {
                out.extend([0, 0, 0, 0]);
            }
        }
    }
    out
}

fn sample(u: f32, v: f32) -> Option<[f32; 3]> {
    if !rounded_rect(u, v, 0.5, 0.5, 0.5, 0.5, 0.22) {
        return None;
    }
    let t = (u + v) / 2.0;
    let brand = [
        lerp(109.0, 59.0, t),
        lerp(93.0, 130.0, t),
        lerp(252.0, 246.0, t),
    ];

    let dot = |cx: f32| (u - cx).powi(2) + (v - 0.45).powi(2) < 0.058f32.powi(2);
    if dot(0.35) || dot(0.5) || dot(0.65) {
        return Some(brand);
    }
    let bubble = rounded_rect(u, v, 0.5, 0.45, 0.31, 0.22, 0.14)
        || in_triangle(u, v, (0.27, 0.6), (0.45, 0.62), (0.24, 0.82));
    Some(if bubble { [255.0, 255.0, 255.0] } else { brand })
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn rounded_rect(u: f32, v: f32, cx: f32, cy: f32, hw: f32, hh: f32, r: f32) -> bool {
    let dx = ((u - cx).abs() - hw + r).max(0.0);
    let dy = ((v - cy).abs() - hh + r).max(0.0);
    (u - cx).abs() <= hw && (v - cy).abs() <= hh && dx * dx + dy * dy <= r * r
}

fn in_triangle(u: f32, v: f32, a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let side = |p: (f32, f32), q: (f32, f32)| (q.0 - p.0) * (v - p.1) - (q.1 - p.1) * (u - p.0);
    let (d1, d2, d3) = (side(a, b), side(b, c), side(c, a));
    !((d1 < 0.0 || d2 < 0.0 || d3 < 0.0) && (d1 > 0.0 || d2 > 0.0 || d3 > 0.0))
}

pub fn png(size: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut bytes, size, size);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(&rgba(size)).expect("png data");
    }
    bytes
}

#[cfg(test)]
mod tests {
    /// Writes the icon at 256px for building hey.ico:
    /// `HEY_ICON_OUT=icon-256.png cargo test write_icon -- --ignored`
    #[test]
    #[ignore]
    fn write_icon() {
        let out = std::env::var("HEY_ICON_OUT").unwrap_or_else(|_| "icon-256.png".into());
        std::fs::write(out, super::png(256)).unwrap();
    }
}
