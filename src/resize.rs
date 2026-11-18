//! Dependency-free Lanczos3 resampling — the other half of "make this image
//! small": for photographic content, downsampling decides more of the final
//! size/quality trade than the encoder does.
//!
//! Separable two-pass resampling (horizontal, then vertical) with
//! precomputed normalized weights per destination index. Intermediates stay
//! f32 so rounding and clamping happen exactly once, at the end.

const LOBES: f32 = 3.0;

fn lanczos3(x: f32) -> f32 {
    let ax = x.abs();
    if ax < 1e-6 {
        return 1.0;
    }
    if ax >= LOBES {
        return 0.0;
    }
    let p = std::f32::consts::PI * x;
    LOBES * p.sin() * (p / LOBES).sin() / (p * p)
}

struct Contrib {
    start: usize,
    weights: Vec<f32>,
}

/// Weights mapping a source axis of length `src` onto `dst` samples.
/// Pixel-center convention; when downscaling the kernel widens by the
/// scale factor so it keeps covering the same source area.
fn contributions(src: usize, dst: usize) -> Vec<Contrib> {
    let ratio = src as f32 / dst as f32;
    let scale = ratio.max(1.0);
    let support = LOBES * scale;
    (0..dst)
        .map(|d| {
            let center = (d as f32 + 0.5) * ratio - 0.5;
            let lo = ((center - support).ceil() as i64).clamp(0, src as i64 - 1) as usize;
            let hi = ((center + support).floor() as i64).clamp(0, src as i64 - 1) as usize;
            let mut weights: Vec<f32> = (lo..=hi)
                .map(|i| lanczos3((i as f32 - center) / scale))
                .collect();
            let sum: f32 = weights.iter().sum();
            weights.iter_mut().for_each(|w| *w /= sum);
            Contrib { start: lo, weights }
        })
        .collect()
}

/// sRGB transfer function, 8-bit code to linear light in 0..=1.
fn srgb_to_linear(v: u8) -> f32 {
    let c = v as f32 / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Inverse of [`srgb_to_linear`], rounded and clamped to an 8-bit code.
fn linear_to_srgb(l: f32) -> u8 {
    let l = l.clamp(0.0, 1.0);
    let c = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round() as u8
}

fn resize_plane<const N: usize>(
    src: &[u8],
    sw: usize,
    sh: usize,
    dw: usize,
    dh: usize,
    linear: bool,
) -> Vec<u8> {
    // Sample value per 8-bit code: the code itself, or linear light.
    let mut lut = [0f32; 256];
    for (v, l) in lut.iter_mut().enumerate() {
        *l = if linear {
            srgb_to_linear(v as u8)
        } else {
            v as f32
        };
    }
    // Horizontal pass: (sw x sh) -> (dw x sh), f32.
    let cx = contributions(sw, dw);
    let mut tmp = vec![0f32; dw * sh * N];
    for y in 0..sh {
        let row = &src[y * sw * N..][..sw * N];
        let out = &mut tmp[y * dw * N..][..dw * N];
        for (d, c) in cx.iter().enumerate() {
            let mut acc = [0f32; N];
            for (k, &w) in c.weights.iter().enumerate() {
                let px = &row[(c.start + k) * N..][..N];
                for ch in 0..N {
                    acc[ch] += w * lut[px[ch] as usize];
                }
            }
            out[d * N..][..N].copy_from_slice(&acc);
        }
    }

    // Vertical pass: accumulate whole rows for cache-friendly access.
    let cy = contributions(sh, dh);
    let mut out = vec![0u8; dw * dh * N];
    let mut accrow = vec![0f32; dw * N];
    for (dy, c) in cy.iter().enumerate() {
        accrow.fill(0.0);
        for (k, &w) in c.weights.iter().enumerate() {
            let row = &tmp[(c.start + k) * dw * N..][..dw * N];
            for (a, &v) in accrow.iter_mut().zip(row) {
                *a += w * v;
            }
        }
        for (o, &a) in out[dy * dw * N..][..dw * N].iter_mut().zip(accrow.iter()) {
            *o = if linear {
                linear_to_srgb(a)
            } else {
                a.round().clamp(0.0, 255.0) as u8
            };
        }
    }
    out
}

/// Resize interleaved 8-bit RGB pixels with a Lanczos3 filter.
///
/// Memory grows with `dw * max(sh, dh)`; bound the sizes when they come
/// from untrusted input.
///
/// # Panics
///
/// Panics if `src.len()` is not exactly `sw * sh * 3`, or if any of the four
/// dimensions is 0.
pub fn resize_rgb(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize * 3);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    resize_plane::<3>(
        src,
        sw as usize,
        sh as usize,
        dw as usize,
        dh as usize,
        false,
    )
}

/// Resize interleaved 8-bit sRGB pixels with a Lanczos3 filter applied in
/// linear light: samples are decoded with the sRGB transfer function,
/// filtered, and encoded back.
///
/// [`resize_rgb`] filters the stored code values, which is what most
/// software does but averages brightness incorrectly: fine light-and-dark
/// detail (text, hair, foliage, star fields) comes out darker than the
/// original looks from a distance. Filtering in linear light keeps the
/// brightness right. It takes about a quarter more time (measured
/// reducing 12 megapixels to 2), and ringing next to hard edges lands on
/// the dark side, where it is easier to see — for screenshots and line art
/// [`resize_rgb`] can be the better pick.
///
/// Assumes sRGB-encoded input (true of most JPEGs; an embedded ICC profile
/// with a different transfer curve is not consulted).
///
/// Memory grows with `dw * max(sh, dh)`; bound the sizes when they come
/// from untrusted input.
///
/// # Panics
///
/// Panics if `src.len()` is not exactly `sw * sh * 3`, or if any of the four
/// dimensions is 0.
pub fn resize_rgb_linear(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize * 3);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    resize_plane::<3>(
        src,
        sw as usize,
        sh as usize,
        dw as usize,
        dh as usize,
        true,
    )
}

/// Resize 8-bit grayscale pixels with a Lanczos3 filter in linear light,
/// treating the values as sRGB-encoded; see [`resize_rgb_linear`].
///
/// Memory grows with `dw * max(sh, dh)`; bound the sizes when they come
/// from untrusted input.
///
/// # Panics
///
/// Panics if `src.len()` is not exactly `sw * sh`, or if any of the four
/// dimensions is 0.
pub fn resize_gray_linear(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    resize_plane::<1>(
        src,
        sw as usize,
        sh as usize,
        dw as usize,
        dh as usize,
        true,
    )
}

/// Resize 8-bit grayscale pixels with a Lanczos3 filter.
///
/// Memory grows with `dw * max(sh, dh)`; bound the sizes when they come
/// from untrusted input.
///
/// # Panics
///
/// Panics if `src.len()` is not exactly `sw * sh`, or if any of the four
/// dimensions is 0.
pub fn resize_gray(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    resize_plane::<1>(
        src,
        sw as usize,
        sh as usize,
        dw as usize,
        dh as usize,
        false,
    )
}

/// Resize interleaved 8-bit four-channel pixels (CMYK, or any other
/// four-channel layout) with a Lanczos3 filter. Every channel is filtered
/// independently and linearly, so the channel values need no
/// interpretation: inverted ("Adobe") CMYK stays inverted.
///
/// Memory grows with `dw * max(sh, dh)`; bound the sizes when they come
/// from untrusted input.
///
/// # Panics
///
/// Panics if `src.len()` is not exactly `sw * sh * 4`, or if any of the four
/// dimensions is 0.
pub fn resize_cmyk(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    assert_eq!(src.len(), sw as usize * sh as usize * 4);
    assert!(sw > 0 && sh > 0 && dw > 0 && dh > 0);
    resize_plane::<4>(
        src,
        sw as usize,
        sh as usize,
        dw as usize,
        dh as usize,
        false,
    )
}

/// The largest size that fits inside `max_width` x `max_height` while
/// keeping the aspect ratio of `width` x `height`, for passing to the
/// resize functions. Never enlarges: an image that already fits comes back
/// unchanged. Each side is at least 1.
///
/// # Panics
///
/// Panics if any argument is 0.
pub fn fit_within(width: u32, height: u32, max_width: u32, max_height: u32) -> (u32, u32) {
    assert!(width > 0 && height > 0 && max_width > 0 && max_height > 0);
    if width <= max_width && height <= max_height {
        return (width, height);
    }
    let (w, h, mw, mh) = (
        width as u64,
        height as u64,
        max_width as u64,
        max_height as u64,
    );
    // Width is the binding side when w/h >= mw/mh.
    if w * mh >= h * mw {
        let nh = ((h * mw + w / 2) / w).clamp(1, mh);
        (max_width, nh as u32)
    } else {
        let nw = ((w * mh + h / 2) / h).clamp(1, mw);
        (nw as u32, max_height)
    }
}

/// A rectangle inside an image, in pixels; see [`crop`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width of the rectangle.
    pub width: u32,
    /// Height of the rectangle.
    pub height: u32,
}

/// Copy the rectangle `rect` out of interleaved 8-bit pixels with any
/// channel count. The result is `rect.width * rect.height * channels`
/// bytes.
///
/// Returns `None` if the rectangle is empty or does not lie entirely
/// inside the image.
///
/// # Panics
///
/// Panics if `pixels.len()` is not exactly `width * height * channels`,
/// or if `channels` is 0.
pub fn crop(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    rect: CropRect,
) -> Option<Vec<u8>> {
    assert!(channels > 0);
    let (w, h) = (width as usize, height as usize);
    assert_eq!(pixels.len(), w * h * channels);
    if rect.width == 0
        || rect.height == 0
        || rect.x.checked_add(rect.width)? > width
        || rect.y.checked_add(rect.height)? > height
    {
        return None;
    }
    let (x, y, cw, ch) = (
        rect.x as usize,
        rect.y as usize,
        rect.width as usize,
        rect.height as usize,
    );
    let mut out = Vec::with_capacity(cw * ch * channels);
    for row in y..y + ch {
        let start = (row * w + x) * channels;
        out.extend_from_slice(&pixels[start..start + cw * channels]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x: u32, y: u32, width: u32, height: u32) -> CropRect {
        CropRect {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn crop_copies_the_rectangle() {
        let (w, h) = (7u32, 5u32);
        let src: Vec<u8> = (0..w * h * 3).map(|i| i as u8).collect();
        let out = crop(&src, w, h, 3, r(2, 1, 4, 3)).unwrap();
        assert_eq!(out.len(), 4 * 3 * 3);
        for cy in 0..3u32 {
            for cx in 0..4u32 {
                let s = (((cy + 1) * w + cx + 2) * 3) as usize;
                let d = ((cy * 4 + cx) * 3) as usize;
                assert_eq!(out[d..d + 3], src[s..s + 3]);
            }
        }
        assert_eq!(crop(&src, w, h, 3, r(0, 0, w, h)).unwrap(), src);
    }

    #[test]
    fn crop_rejects_bad_rectangles() {
        let src = vec![0u8; 6 * 4];
        assert!(crop(&src, 6, 4, 1, r(0, 0, 0, 2)).is_none());
        assert!(crop(&src, 6, 4, 1, r(0, 0, 2, 0)).is_none());
        assert!(crop(&src, 6, 4, 1, r(5, 0, 2, 1)).is_none());
        assert!(crop(&src, 6, 4, 1, r(0, 3, 1, 2)).is_none());
        assert!(crop(&src, 6, 4, 1, r(u32::MAX, 0, 2, 1)).is_none());
        assert!(crop(&src, 6, 4, 1, r(5, 3, 1, 1)).is_some());
    }

    #[test]
    fn fit_within_keeps_aspect_and_never_enlarges() {
        assert_eq!(fit_within(4000, 3000, 1600, 1600), (1600, 1200));
        assert_eq!(fit_within(3000, 4000, 1600, 1600), (1200, 1600));
        assert_eq!(fit_within(4000, 3000, 1000, 100), (133, 100));
        assert_eq!(fit_within(800, 600, 1600, 1600), (800, 600));
        assert_eq!(fit_within(10000, 1, 100, 100), (100, 1));
        assert_eq!(fit_within(1, 10000, 100, 100), (1, 100));
        assert_eq!(fit_within(u32::MAX, u32::MAX, u32::MAX - 1, 7), (7, 7));
    }

    #[test]
    fn srgb_round_trip_is_exact() {
        for v in 0..=255u8 {
            assert_eq!(linear_to_srgb(srgb_to_linear(v)), v);
        }
    }

    #[test]
    fn linear_identity_is_exact() {
        let (w, h) = (37u32, 23u32);
        let src: Vec<u8> = (0..w * h * 3).map(|i| (i * 31 % 256) as u8).collect();
        assert_eq!(resize_rgb_linear(&src, w, h, w, h), src);
    }

    #[test]
    fn linear_keeps_checkerboard_brightness() {
        // A black/white pixel checkerboard reflects half the light of white.
        // Averaging code values gives 128; half the light is sRGB 188.
        let (w, h) = (64u32, 64u32);
        let src: Vec<u8> = (0..w * h)
            .map(|i| if (i % w + i / w) % 2 == 0 { 255 } else { 0 })
            .collect();
        let center = |v: Vec<u8>| v[8 * 16 + 8];
        let plain = center(resize_gray(&src, w, h, 16, 16));
        let linear = center(resize_gray_linear(&src, w, h, 16, 16));
        assert!((126..=130).contains(&plain), "plain {plain}");
        assert!((186..=190).contains(&linear), "linear {linear}");
    }

    #[test]
    fn identity_is_exact() {
        // At 1:1 the kernel hits integer offsets where lanczos3 is 0 except
        // at the center, so the image must come back byte-identical.
        let (w, h) = (37u32, 23u32);
        let src: Vec<u8> = (0..w * h * 3).map(|i| (i * 31 % 256) as u8).collect();
        assert_eq!(resize_rgb(&src, w, h, w, h), src);
    }

    #[test]
    fn flat_stays_flat() {
        let src = vec![137u8; 64 * 48];
        let out = resize_gray(&src, 64, 48, 17, 11);
        assert!(out.iter().all(|&v| v == 137), "normalization must hold");
    }

    #[test]
    fn matches_image_crate_lanczos3() {
        let (w, h) = (200u32, 130u32);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                rgb.push(((x * 255 / w) ^ (y * 3)) as u8);
                rgb.push(((x as f32 / 7.0).sin() * 90.0 + 128.0) as u8);
                rgb.push(((x + y) * 255 / (w + h)) as u8);
            }
        }
        let (dw, dh) = (77u32, 53u32);
        let ours = resize_rgb(&rgb, w, h, dw, dh);

        let img = image::RgbImage::from_raw(w, h, rgb).unwrap();
        let theirs = image::DynamicImage::ImageRgb8(img)
            .resize_exact(dw, dh, image::imageops::FilterType::Lanczos3)
            .to_rgb8();

        let mut max_d = 0i32;
        let mut sum_d = 0f64;
        for (&a, &b) in ours.iter().zip(theirs.as_raw()) {
            let d = (a as i32 - b as i32).abs();
            max_d = max_d.max(d);
            sum_d += d as f64;
        }
        let mean_d = sum_d / ours.len() as f64;
        assert!(
            mean_d < 1.0 && max_d <= 4,
            "diverges from image crate: mean {mean_d:.3}, max {max_d}"
        );
    }
}
