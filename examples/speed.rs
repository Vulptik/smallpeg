//! Times the main entry points on a synthetic photo-like image and prints
//! one `name milliseconds` line per task (the fastest of `RUNS` runs,
//! default 5). CI runs it against a pull request and its base on the same
//! machine to catch slowdowns: `cargo run --release --example speed`.

use std::hint::black_box;
use std::time::Instant;

/// Smooth shading, edges and grain, roughly like a photo: deterministic,
/// so every run and every build sees the same bytes.
fn photo(w: u32, h: u32) -> Vec<u8> {
    let mut seed = 0x2545_f491_4f6c_dd1du64;
    let mut px = Vec::with_capacity((w * h * 3) as usize);
    for y in 0..h {
        for x in 0..w {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let grain = (seed % 24) as i32 - 12;
            let (fx, fy) = (x as f32 / w as f32, y as f32 / h as f32);
            let wave = ((fx * 23.0).sin() * (fy * 17.0).cos() * 60.0) as i32;
            let edge = if (x / 97 + y / 61) % 3 == 0 { 70 } else { 0 };
            let base = [
                (fx * 200.0) as i32 + wave,
                (fy * 180.0) as i32 + edge,
                ((1.0 - fx) * 160.0) as i32 - wave / 2,
            ];
            for b in base {
                px.push((b + 30 + grain).clamp(0, 255) as u8);
            }
        }
    }
    px
}

fn time(name: &str, runs: usize, mut f: impl FnMut()) {
    let mut best = f64::MAX;
    for _ in 0..runs {
        let t = Instant::now();
        f();
        best = best.min(t.elapsed().as_secs_f64() * 1000.0);
    }
    println!("{name} {best:.1}");
}

fn main() {
    let runs = std::env::var("RUNS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(5);
    let (w, h) = (2048, 1536);
    let rgb = photo(w, h);
    let progressive = smallpeg::Encoder::new(80).encode_rgb(&rgb, w, h);
    let baseline = smallpeg::Encoder::new(80)
        .progressive(false)
        .encode_rgb(&rgb, w, h);

    time("encode_progressive", runs, || {
        black_box(smallpeg::Encoder::new(80).encode_rgb(black_box(&rgb), w, h));
    });
    time("encode_baseline", runs, || {
        black_box(
            smallpeg::Encoder::new(80)
                .progressive(false)
                .encode_rgb(black_box(&rgb), w, h),
        );
    });
    time("optimize", runs, || {
        black_box(smallpeg::optimize_jpeg(black_box(&baseline)));
    });
    time("decode", runs, || {
        black_box(smallpeg::decode_jpeg(black_box(&progressive)));
    });
    time("resize", runs, || {
        black_box(smallpeg::resize_rgb_linear(
            black_box(&rgb),
            w,
            h,
            w / 2,
            h / 2,
        ));
    });
}
