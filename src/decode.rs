//! Pixel decoding on top of the entropy decoder in `optimize`: inverse DCT,
//! upsampling and color conversion (clean-room from ITU T.81 and the JFIF
//! and Adobe conventions mainstream decoders follow).
//!
//! Upsampling of subsampled components is linear interpolation between
//! sample centers (weights 3/4 and 1/4 at 2x), the filter mainstream
//! decoders apply by default, so decoded images match mainstream decoders rather
//! than showing replication blockiness at chroma edges.

use crate::optimize::{Limits, Parsed, parse_and_decode};
use crate::quant::ZIGZAG;

/// A decoded grayscale or RGB JPEG; see [`decode_jpeg`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedImage {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// 1 (grayscale) or 3 (RGB).
    pub channels: u8,
    /// `width * height * channels` bytes, interleaved, top row first.
    pub pixels: Vec<u8>,
}

/// Decode a grayscale or color JPEG to 8-bit gray or RGB pixels.
///
/// Three-component files are converted from YCbCr to RGB unless they say
/// they hold RGB directly, decided the conventional way: a JFIF
/// segment means YCbCr; otherwise an Adobe APP14 with transform 0 means
/// RGB; otherwise component ids `'R'`, `'G'`, `'B'` mean RGB; anything else
/// is YCbCr. No color management: an embedded ICC profile is not applied.
///
/// Returns `None` for four-component files (see
/// [`decode_cmyk`](crate::decode_cmyk)), for anything that is not a
/// baseline or progressive Huffman-coded 8-bit JPEG, and for malformed
/// input. Like [`optimize_jpeg`](crate::optimize_jpeg), it takes untrusted
/// input and never panics. Uses the default [`Limits`] (100 megapixels,
/// 128 scans); [`decode_jpeg_with`] takes others.
pub fn decode_jpeg(jpeg: &[u8]) -> Option<DecodedImage> {
    decode_jpeg_with(jpeg, &Limits::default())
}

/// [`decode_jpeg`] with caller-chosen [`Limits`]; `None` also when the
/// input exceeds them.
pub fn decode_jpeg_with(jpeg: &[u8], limits: &Limits) -> Option<DecodedImage> {
    let p = parse_and_decode(jpeg, true, limits)?;
    let (w, h) = (p.width as usize, p.height as usize);
    match p.comps.len() {
        1 => {
            let planes = sample_planes(&p)?;
            let mut up = Upsampler::new(&planes[0], &p, 0);
            let mut pixels = vec![0u8; w * h];
            for (y, o) in pixels.chunks_exact_mut(w).enumerate() {
                up.advance(y);
                o.copy_from_slice(up.row());
            }
            Some(DecodedImage {
                width: w as u32,
                height: h as u32,
                channels: 1,
                pixels,
            })
        }
        3 => {
            let rgb_direct = !p.jfif
                && match p.adobe {
                    Some(t) => t == 0,
                    None => p.ids == *b"RGB",
                };
            let planes = sample_planes(&p)?;
            let mut ups: Vec<Upsampler> =
                (0..3).map(|i| Upsampler::new(&planes[i], &p, i)).collect();
            let mut pixels = vec![0u8; w * h * 3];
            for (y, o) in pixels.chunks_exact_mut(w * 3).enumerate() {
                ups.iter_mut().for_each(|u| u.advance(y));
                let (r0, r1, r2) = (ups[0].row(), ups[1].row(), ups[2].row());
                let o = o.as_chunks_mut::<3>().0;
                let src = r0.iter().zip(r1).zip(r2);
                if rgb_direct {
                    for (px, ((&a, &b), &c)) in o.iter_mut().zip(src) {
                        *px = [a, b, c];
                    }
                } else {
                    ycc_row(r0, r1, r2, o);
                }
            }
            Some(DecodedImage {
                width: w as u32,
                height: h as u32,
                channels: 3,
                pixels,
            })
        }
        _ => None,
    }
}

/// YCbCr (JFIF, full range) to RGB for one row:
/// R = Y + 1.402 Cr', G = Y - 0.344136 Cb' - 0.714136 Cr',
/// B = Y + 1.772 Cb' (Cb' = Cb - 128), in fixed point with 16 fractional
/// bits, rounded half up, clamped. No table lookups, so it vectorizes.
pub(crate) fn ycc_row(y: &[u8], cb: &[u8], cr: &[u8], out: &mut [[u8; 3]]) {
    const R_CR: i32 = 91881; // 1.402
    const G_CB: i32 = -22554; // -0.344136
    const G_CR: i32 = -46802; // -0.714136
    const B_CB: i32 = 116130; // 1.772
    const HALF: i32 = 1 << 15;
    for (o, ((&yy, &b), &r)) in out.iter_mut().zip(y.iter().zip(cb).zip(cr)) {
        let (yy, b, r) = (yy as i32, b as i32 - 128, r as i32 - 128);
        *o = [
            (yy + ((R_CR * r + HALF) >> 16)).clamp(0, 255) as u8,
            (yy + ((G_CB * b + G_CR * r + HALF) >> 16)).clamp(0, 255) as u8,
            (yy + ((B_CB * b + HALF) >> 16)).clamp(0, 255) as u8,
        ];
    }
}

/// One component's decoded samples on its padded block grid.
pub(crate) struct Plane {
    data: Vec<u8>,
    stride: usize,
}

/// Inverse-DCT every component into its sample plane. `None` if a
/// component references a quantization table the file never defined.
pub(crate) fn sample_planes(p: &Parsed) -> Option<Vec<Plane>> {
    let basis = idct_basis();
    let mut planes = Vec::with_capacity(p.comps.len());
    for c in &p.comps {
        let qt = p.qts[c.tq as usize].as_ref()?;
        let stride = c.bw * 8;
        let bh = c.blocks.len() / c.bw;
        let mut data = vec![0u8; stride * bh * 8];
        let mut f = [0f32; 64];
        for (bi, blk) in c.blocks.iter().enumerate() {
            let (bx, by) = (bi % c.bw, bi / c.bw);
            let at = by * 8 * stride + bx * 8;
            if blk[1..].iter().all(|&v| v == 0) {
                // DC only (most blocks of flat content): a constant block,
                // DC basis C(0)^2 / 4 = 1/8.
                let v = (blk[0] as f32 * qt[0] as f32 / 8.0 + 128.0)
                    .round()
                    .clamp(0.0, 255.0) as u8;
                for y in 0..8 {
                    data[at + y * stride..][..8].fill(v);
                }
                continue;
            }
            for k in 0..64 {
                f[ZIGZAG[k]] = blk[k] as f32 * qt[k] as f32;
            }
            let px = idct8x8(&f, &basis);
            for y in 0..8 {
                let row = &mut data[at + y * stride..][..8];
                for x in 0..8 {
                    row[x] = (px[y * 8 + x] + 128.0).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        planes.push(Plane { data, stride });
    }
    Some(planes)
}

/// Produces one component's samples at full image resolution, one output
/// row at a time. Subsampled components are interpolated linearly between
/// sample centers; samples beyond the component's real extent (block
/// padding) are never read.
pub(crate) struct Upsampler<'a> {
    plane: &'a Plane,
    w: usize,
    /// Real sample extent of the component (T.81 A.1.1).
    cw: usize,
    ch: usize,
    mode: Mode,
    /// Output row (unused in `Mode::Full`).
    buf: Vec<u8>,
    /// Column sums (`Mode::Int`) carry a factor of 4.
    cs: Vec<u16>,
    rowf: Vec<f32>,
    /// Current row index for `Mode::Full`.
    y: usize,
}

enum Mode {
    /// Full resolution: rows are read from the plane as they are.
    Full,
    /// 1x or 2x per axis (4:2:0, 4:2:2, 4:4:0): 3/4 + 1/4 weights in
    /// integers.
    Int { fx: usize, fy: usize },
    /// Any other ratio: per-axis taps (lower, upper, weight of upper).
    Taps {
        tx: Vec<(usize, usize, f32)>,
        ty: Vec<(usize, usize, f32)>,
    },
}

impl<'a> Upsampler<'a> {
    pub(crate) fn new(plane: &'a Plane, p: &Parsed, ci: usize) -> Self {
        let (w, h) = (p.width as usize, p.height as usize);
        let c = &p.comps[ci];
        let hmax = p.comps.iter().map(|c| c.h).max().unwrap_or(1);
        let vmax = p.comps.iter().map(|c| c.v).max().unwrap_or(1);
        let cw = (w * c.h).div_ceil(hmax);
        let ch = (h * c.v).div_ceil(vmax);
        let (fx, fy) = (hmax / c.h, vmax / c.v);
        let mode = if c.h == hmax && c.v == vmax {
            Mode::Full
        } else if hmax % c.h == 0 && vmax % c.v == 0 && fx <= 2 && fy <= 2 {
            Mode::Int { fx, fy }
        } else {
            let taps = |n: usize, num: usize, den: usize, len: usize| {
                (0..n)
                    .map(|x| {
                        let s = (x as f32 + 0.5) * num as f32 / den as f32 - 0.5;
                        let s = s.clamp(0.0, (len - 1) as f32);
                        let i0 = s.floor() as usize;
                        (i0, (i0 + 1).min(len - 1), s - i0 as f32)
                    })
                    .collect()
            };
            Mode::Taps {
                tx: taps(w, c.h, hmax, cw),
                ty: taps(h, c.v, vmax, ch),
            }
        };
        Upsampler {
            plane,
            w,
            cw,
            ch,
            mode,
            buf: vec![0u8; w],
            cs: vec![0u16; cw],
            rowf: vec![0f32; cw],
            y: 0,
        }
    }

    fn src_row(&self, i: usize) -> &'a [u8] {
        &self.plane.data[i * self.plane.stride..][..self.cw]
    }

    /// Compute output row `y`; read it back with `row`.
    pub(crate) fn advance(&mut self, y: usize) {
        let (w, cw, ch) = (self.w, self.cw, self.ch);
        match &self.mode {
            Mode::Full => self.y = y,
            &Mode::Int { fx, fy } => {
                let near = self.src_row(y / fy);
                if fy == 2 {
                    let i = y / 2;
                    let nb = if y.is_multiple_of(2) {
                        i.saturating_sub(1)
                    } else {
                        (i + 1).min(ch - 1)
                    };
                    let far = self.src_row(nb);
                    for ((s, &a), &b) in self.cs.iter_mut().zip(near).zip(far) {
                        *s = 3 * a as u16 + b as u16;
                    }
                } else {
                    for (s, &a) in self.cs.iter_mut().zip(near) {
                        *s = 4 * a as u16;
                    }
                }
                let cs = &self.cs;
                if fx == 2 {
                    // Output pair 2j, 2j+1 leans toward sample j-1 / j+1.
                    // Edges replicate; the interior runs over windows with
                    // no index arithmetic or bounds checks.
                    let pair = |l: u16, c: u16, r: u16| {
                        let c3 = 3 * c as u32 + 8;
                        [((c3 + l as u32) >> 4) as u8, ((c3 + r as u32) >> 4) as u8]
                    };
                    let mut tmp = [0u8; 2];
                    let (body, _) = self.buf.as_chunks_mut::<2>();
                    if cw == 1 {
                        // Width 1 or 2: one sample covers everything.
                        tmp = pair(cs[0], cs[0], cs[0]);
                        if let Some(b) = body.first_mut() {
                            *b = tmp;
                        }
                    } else {
                        body[0] = pair(cs[0], cs[0], cs[1]);
                        for (o, win) in body[1..].iter_mut().zip(cs.windows(3)) {
                            *o = pair(win[0], win[1], win[2]);
                        }
                        let last = pair(cs[cw - 2], cs[cw - 1], cs[cw - 1]);
                        if cw - 1 < body.len() {
                            body[cw - 1] = last;
                        } else {
                            tmp = last;
                        }
                    }
                    // Odd width: the final column is the first of a pair
                    // that has no room in `body`.
                    if w % 2 == 1 {
                        self.buf[w - 1] = tmp[0];
                    }
                } else {
                    for (d, &s) in self.buf.iter_mut().zip(cs) {
                        *d = ((s as u32 + 2) >> 2) as u8;
                    }
                }
            }
            Mode::Taps { tx, ty } => {
                let (y0, y1, f) = ty[y];
                let (r0, r1) = (self.src_row(y0), self.src_row(y1));
                for ((v, &a), &b) in self.rowf.iter_mut().zip(r0).zip(r1) {
                    *v = a as f32 + (b as f32 - a as f32) * f;
                }
                let row = &self.rowf;
                for (d, &(x0, x1, f)) in self.buf.iter_mut().zip(tx) {
                    *d = (row[x0] + (row[x1] - row[x0]) * f)
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
    }

    /// The row computed by the last `advance`, `width` samples.
    pub(crate) fn row(&self) -> &[u8] {
        match self.mode {
            Mode::Full => &self.plane.data[self.y * self.plane.stride..][..self.w],
            _ => &self.buf,
        }
    }
}

/// `basis[x][u] = C(u) / 2 * cos((2x + 1) u pi / 16)`, the 1-D inverse DCT.
fn idct_basis() -> [[f32; 8]; 8] {
    let mut b = [[0f32; 8]; 8];
    for (x, row) in b.iter_mut().enumerate() {
        for (u, v) in row.iter_mut().enumerate() {
            let cu = if u == 0 {
                std::f64::consts::FRAC_1_SQRT_2
            } else {
                1.0
            };
            let a = (2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0;
            *v = (cu / 2.0 * a.cos()) as f32;
        }
    }
    b
}

/// Separable 8x8 inverse DCT of dequantized natural-order coefficients.
/// (Measured: this dot-product form beats an "axpy" row form here; zero
/// coefficient rows are skipped in the first pass.)
pub(crate) fn idct8x8(f: &[f32; 64], basis: &[[f32; 8]; 8]) -> [f32; 64] {
    let mut tmp = [0f32; 64];
    for v in 0..8 {
        let row = &f[v * 8..][..8];
        if row.iter().all(|&c| c == 0.0) {
            continue; // tmp row stays zero
        }
        for x in 0..8 {
            let mut s = 0f32;
            for u in 0..8 {
                s += basis[x][u] * row[u];
            }
            tmp[v * 8 + x] = s;
        }
    }
    let mut out = [0f32; 64];
    for x in 0..8 {
        for y in 0..8 {
            let mut s = 0f32;
            for v in 0..8 {
                s += basis[y][v] * tmp[v * 8 + x];
            }
            out[y * 8 + x] = s;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dct::{dequant_scale, fdct8x8};
    use crate::{Encoder, Sampling};

    fn test_rgb(w: u32, h: u32) -> Vec<u8> {
        let mut v = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                v.push((x * 255 / w) as u8);
                v.push(if (x / 5 + y / 7) % 2 == 0 { 40 } else { 210 });
                v.push(((x * y) % 256) as u8);
            }
        }
        v
    }

    fn diff(a: &[u8], b: &[u8]) -> (u8, f64) {
        let (mut m, mut s) = (0u8, 0f64);
        for (&x, &y) in a.iter().zip(b) {
            m = m.max(x.abs_diff(y));
            s += x.abs_diff(y) as f64;
        }
        (m, s / a.len() as f64)
    }

    #[test]
    fn idct_inverts_the_forward_dct() {
        let mut px = [0f32; 64];
        for (i, v) in px.iter_mut().enumerate() {
            *v = ((i * 37) % 200) as f32 - 100.0;
        }
        let orig = px;
        fdct8x8(&mut px);
        // fdct8x8 leaves AAN scaling in; dequant_scale of an all-ones table
        // holds exactly those factors.
        let s = dequant_scale(&[1u16; 64]);
        let mut f = [0f32; 64];
        for i in 0..64 {
            f[i] = px[i] * s[i];
        }
        let back = idct8x8(&f, &idct_basis());
        for i in 0..64 {
            assert!(
                (back[i] - orig[i]).abs() < 1e-2,
                "{i}: {} vs {}",
                back[i],
                orig[i]
            );
        }
    }

    #[test]
    fn matches_an_independent_decoder() {
        // Our own output and a third-party encoder's, gray and RGB, both
        // samplings, baseline and progressive.
        for &(w, h) in &[(64u32, 48u32), (61, 45), (17, 130)] {
            let rgb = test_rgb(w, h);
            let gray: Vec<u8> = rgb.chunks(3).map(|p| p[1]).collect();
            let mut files: Vec<(String, Vec<u8>)> = Vec::new();
            for s in [Sampling::S444, Sampling::S420] {
                for prog in [false, true] {
                    let e = || Encoder::new(85).sampling(s).progressive(prog);
                    files.push((format!("rgb {s:?} {prog}"), e().encode_rgb(&rgb, w, h)));
                    files.push((format!("gray {prog}"), e().encode_gray(&gray, w, h)));
                }
            }
            let mut je = Vec::new();
            jpeg_encoder::Encoder::new(&mut je, 85)
                .encode(&rgb, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
                .unwrap();
            files.push(("jpeg-encoder".into(), je));
            for (what, j) in files {
                let ours = decode_jpeg(&j).expect("decodes");
                let theirs = image::load_from_memory(&j).unwrap();
                let theirs = if ours.channels == 1 {
                    theirs.to_luma8().into_raw()
                } else {
                    theirs.to_rgb8().into_raw()
                };
                assert_eq!((ours.width, ours.height), (w, h));
                let (m, mean) = diff(&ours.pixels, &theirs);
                assert!(
                    m <= 6 && mean < 0.6,
                    "{what} {w}x{h}: max {m} mean {mean:.3}"
                );
            }
        }
    }

    #[test]
    fn rejects_cmyk_and_garbage_without_panicking() {
        let cmyk = vec![50u8; 16 * 16 * 4];
        let j = Encoder::new(80).encode_cmyk(&cmyk, 16, 16, None);
        assert!(decode_jpeg(&j).is_none());
        assert!(decode_jpeg(b"").is_none());
        let rgb = test_rgb(40, 30);
        let j = Encoder::new(75).progressive(true).encode_rgb(&rgb, 40, 30);
        for n in 0..j.len() {
            let _ = decode_jpeg(&j[..n]);
        }
        let mut seed = 99u64;
        for _ in 0..1000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let i = (seed >> 33) as usize % j.len();
            let mut m = j.clone();
            m[i] ^= (seed >> 17) as u8 | 1;
            let _ = decode_jpeg(&m);
        }
    }
}
