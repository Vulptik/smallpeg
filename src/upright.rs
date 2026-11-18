//! Lossless EXIF orientation: turning a JPEG upright by rearranging its
//! quantized DCT coefficients instead of decoding and re-encoding pixels
//! (clean-room from ITU T.81 and the DCT's symmetries).
//!
//! Mirroring a block negates its odd horizontal (or vertical)
//! frequencies, and transposing a block transposes its coefficients, so
//! every orientation maps whole blocks onto whole blocks with the same
//! quantized values. Only edges that are not a whole MCU wide cannot
//! move: a mirrored axis is trimmed to whole MCUs first.

use crate::ceil_div;
use crate::optimize::{FComp, Limits, Parsed, Placed, encode_parsed, parse_and_decode};
use crate::quant::ZIGZAG;

/// Turn a JPEG upright without decoding it to pixels: the EXIF orientation is
/// applied to the image itself and the tag is set to 1, so every viewer
/// shows it the same way, including those that ignore EXIF. The pixels
/// are the original pixels moved, with no generation loss; the entropy
/// coding is optimized as in [`optimize_jpeg`](crate::optimize_jpeg), so
/// the result may be progressive. For baseline output, pass it to
/// [`optimize_jpeg_baseline`](crate::optimize_jpeg_baseline), which is
/// lossless too.
///
/// Orientations that mirror an axis (2, 3, 4, 6, 7, 8) need that axis to
/// be a whole number of MCUs (8 or 16 pixels in most files); otherwise the
/// last partial row or column of MCUs (up to 15 pixels in most files) is
/// cut off, as it cannot be moved losslessly. [`jpeg_info`](crate::jpeg_info) on the
/// result gives the final size. A phone photo of 4032 x 3024 (orientation
/// 6) keeps every pixel.
///
/// Returns `None` where [`optimize_jpeg`](crate::optimize_jpeg) does, and
/// when trimming would leave nothing. Every EXIF orientation tag in the
/// output is 1; apart from that, a file with orientation 1 (or none)
/// comes out as [`optimize_jpeg`](crate::optimize_jpeg) would write it.
///
/// ```
/// # let jpeg = smallpeg::Encoder::new(80).encode_rgb(&[128; 32 * 16 * 3], 32, 16);
/// if let Some(upright) = smallpeg::upright_jpeg(&jpeg) {
///     assert_eq!(smallpeg::jpeg_info(&upright).unwrap().orientation, 1);
/// }
/// ```
pub fn upright_jpeg(data: &[u8]) -> Option<Vec<u8>> {
    upright_jpeg_with(data, &Limits::default())
}

/// [`upright_jpeg`] with caller-chosen [`Limits`]; `None` also when the
/// input exceeds them.
pub fn upright_jpeg_with(data: &[u8], limits: &Limits) -> Option<Vec<u8>> {
    let p = parse_and_decode(data, false, limits)?;

    // The orientation that viewers apply: the first valid one before the
    // frame header, as in `jpeg_info`.
    let o = p
        .meta
        .iter()
        .filter(|r| r.start < p.frame_at && data[r.start + 1] == 0xE1)
        .find_map(|r| crate::meta::exif_orientation(&data[r.start + 4..r.end]))
        .unwrap_or(1);
    // (transpose, mirror the source x axis, mirror the source y axis):
    // output pixel (x, y) is source pixel (x, y) or (y, x), mirrored.
    let (t, fx, fy) = match o {
        2 => (false, true, false),
        3 => (false, true, true),
        4 => (false, false, true),
        5 => (true, false, false),
        6 => (true, false, true),
        7 => (true, true, true),
        8 => (true, true, false),
        _ => (false, false, false),
    };

    // Every orientation tag copied out becomes 1, wherever it is: the
    // output is upright, and readers differ on which tags they honor.
    let placed = Placed::new(
        data,
        &p,
        |seg| {
            if seg[1] == 0xE1
                && let Some((_, pos, le)) = crate::meta::exif_orientation_at(&seg[4..])
            {
                let one: [u8; 2] = if le { [1, 0] } else { [0, 1] };
                seg[4 + pos..4 + pos + 2].copy_from_slice(&one);
            }
        },
        |seg, out| {
            if t {
                out.extend_from_slice(&transpose_dqt(seg));
            } else {
                out.extend_from_slice(seg);
            }
        },
    );

    let p = if (t, fx, fy) == (false, false, false) {
        p
    } else {
        transform(p, t, fx, fy)?
    };
    Some(encode_parsed(placed, p, true))
}

/// Zigzag position of each natural-order index.
fn unzigzag() -> [usize; 64] {
    let mut u = [0usize; 64];
    for (k, &n) in ZIGZAG.iter().enumerate() {
        u[n] = k;
    }
    u
}

/// A DQT segment (marker included, structure already validated by the
/// parser) with every table transposed.
fn transpose_dqt(seg: &[u8]) -> Vec<u8> {
    let un = unzigzag();
    let mut out = seg.to_vec();
    let mut p = 4usize;
    while p < seg.len() {
        let size = if seg[p] >> 4 == 0 { 1 } else { 2 };
        for k in 0..64 {
            let n = ZIGZAG[k];
            let src = un[(n % 8) * 8 + n / 8];
            let (d, s) = (p + 1 + size * k, p + 1 + size * src);
            out[d..d + size].copy_from_slice(&seg[s..s + size]);
        }
        p += 1 + 64 * size;
    }
    out
}

/// Move every block of `p` to its oriented place. `None` when a mirrored
/// axis has no whole MCU.
fn transform(p: Parsed, t: bool, fx: bool, fy: bool) -> Option<Parsed> {
    let hmax = p.comps.iter().map(|c| c.h).max().unwrap_or(1);
    let vmax = p.comps.iter().map(|c| c.v).max().unwrap_or(1);
    // Source size after trimming mirrored axes to whole MCUs.
    let (mut sw, mut sh) = (p.width as usize, p.height as usize);
    if fx {
        sw -= sw % (8 * hmax);
    }
    if fy {
        sh -= sh % (8 * vmax);
    }
    if sw == 0 || sh == 0 {
        return None;
    }
    let (w, h) = if t { (sh, sw) } else { (sw, sh) };
    let (ohmax, ovmax) = if t { (vmax, hmax) } else { (hmax, vmax) };
    let mcu_w = ceil_div(w, 8 * ohmax);
    let mcu_h = ceil_div(h, 8 * ovmax);

    // Per zigzag position of the output: the source zigzag position and
    // whether the value changes sign. Mirroring x negates odd horizontal
    // frequencies of the source block, mirroring y odd vertical ones.
    let un = unzigzag();
    let mut map = [(0usize, false); 64];
    for (k, m) in map.iter_mut().enumerate() {
        let (r, c) = (ZIGZAG[k] / 8, ZIGZAG[k] % 8);
        let (sr, sc) = if t { (c, r) } else { (r, c) };
        *m = (un[sr * 8 + sc], (fx && sc % 2 == 1) ^ (fy && sr % 2 == 1));
    }

    let mut comps = Vec::with_capacity(p.comps.len());
    for c in p.comps {
        let (oh, ov) = if t { (c.v, c.h) } else { (c.h, c.v) };
        let (bw, bh) = (mcu_w * oh, mcu_h * ov);
        // Source block columns and rows that hold image data after
        // trimming; a mirrored axis is a whole number of MCUs, so exact.
        let sbw = ceil_div(ceil_div(sw * c.h, hmax), 8);
        let sbh = ceil_div(ceil_div(sh * c.v, vmax), 8);
        let src_rows = c.blocks.len() / c.bw;
        let mut blocks = vec![[0i16; 64]; bw * bh];
        for by in 0..bh {
            for bx in 0..bw {
                let (mut ix, mut iy) = if t { (by, bx) } else { (bx, by) };
                if fx {
                    let Some(m) = sbw.checked_sub(1 + ix) else {
                        continue;
                    };
                    ix = m;
                }
                if fy {
                    let Some(m) = sbh.checked_sub(1 + iy) else {
                        continue;
                    };
                    iy = m;
                }
                if ix >= c.bw || iy >= src_rows {
                    continue;
                }
                let src = &c.blocks[iy * c.bw + ix];
                let dst = &mut blocks[by * bw + bx];
                for (d, &(s, neg)) in dst.iter_mut().zip(&map) {
                    *d = if neg { -src[s] } else { src[s] };
                }
            }
        }
        comps.push(FComp {
            h: oh,
            v: ov,
            tq: c.tq,
            bw,
            aw: ceil_div(ceil_div(w * oh, ohmax), 8),
            ah: ceil_div(ceil_div(h * ov, ovmax), 8),
            blocks,
        });
    }
    Some(Parsed {
        width: w as u16,
        height: h as u16,
        comps,
        mcu_w,
        mcu_h,
        ..p
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Encoder, Sampling, apply_orientation, crop, decode_jpeg, jpeg_info};

    /// A TIFF stream with only an Orientation tag: little-endian for odd
    /// orientations, big-endian for even ones.
    fn tiff(o: u8) -> Vec<u8> {
        if o % 2 == 1 {
            vec![
                b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 0x01, 3, 0, 1, 0, 0, 0, o, 0, 0, 0, 0,
                0, 0, 0,
            ]
        } else {
            vec![
                b'M', b'M', 0, 42, 0, 0, 0, 8, 0, 1, 0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, o, 0, 0, 0,
                0, 0, 0,
            ]
        }
    }

    /// The orientation in the first EXIF segment, read directly.
    fn exif_tag(j: &[u8]) -> Option<u8> {
        let at = j
            .windows(10)
            .position(|w| w[..2] == [0xFF, 0xE1] && &w[4..] == b"Exif\0\0")?;
        let len = u16::from_be_bytes([j[at + 2], j[at + 3]]) as usize;
        crate::meta::exif_orientation(&j[at + 4..at + 2 + len])
    }

    fn rgb(w: u32, h: u32) -> Vec<u8> {
        // Smooth gradients plus an asymmetric mark, so a wrong turn shows.
        let mut v = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                let mark = x < w / 3 && y < h / 4;
                v.push((x * 255 / w) as u8);
                v.push((y * 255 / h) as u8);
                v.push(if mark { 230 } else { 40 });
            }
        }
        v
    }

    /// Source JPEGs: (bytes, MCU width, MCU height).
    fn sources(w: u32, h: u32, o: u8) -> Vec<(Vec<u8>, u32, u32)> {
        let px = rgb(w, h);
        let gray: Vec<u8> = px.chunks(3).map(|p| p[0]).collect();
        let mut out = Vec::new();
        for (s, m) in [(Sampling::S444, 8), (Sampling::S420, 16)] {
            for prog in [false, true] {
                let j = Encoder::new(90)
                    .sampling(s)
                    .progressive(prog)
                    .exif(&tiff(o))
                    .encode_rgb(&px, w, h);
                out.push((j, m, m));
            }
        }
        out.push((
            Encoder::new(90).exif(&tiff(o)).encode_gray(&gray, w, h),
            8,
            8,
        ));
        // 4:2:2 (MCU 16 x 8) from another encoder.
        let mut buf = Vec::new();
        let mut e = jpeg_encoder::Encoder::new(&mut buf, 90);
        e.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_2_2);
        let mut app1 = b"Exif\0\0".to_vec();
        app1.extend_from_slice(&tiff(o));
        e.add_app_segment(1, app1).unwrap();
        e.encode(&px, w as u16, h as u16, jpeg_encoder::ColorType::Rgb)
            .unwrap();
        out.push((buf, 16, 8));
        out
    }

    #[test]
    fn exif_after_the_frame_header_is_ignored() {
        // Viewers and `jpeg_info` stop reading tags at SOF, so a tag
        // placed later must not turn the image either.
        let j = Encoder::new(80).encode_rgb(&rgb(48, 32), 48, 32);
        let sof = j
            .windows(2)
            .position(|w| w == [0xFF, 0xC0] || w == [0xFF, 0xC2])
            .unwrap();
        let sos = j.windows(2).position(|w| w == [0xFF, 0xDA]).unwrap();
        let mut app1 = vec![0xFF, 0xE1, 0, 0];
        app1.extend_from_slice(b"Exif\0\0");
        app1.extend_from_slice(&tiff(6));
        let len = (app1.len() - 2) as u16;
        app1[2..4].copy_from_slice(&len.to_be_bytes());
        let mut late = j[..sos].to_vec();
        late.extend_from_slice(&app1);
        late.extend_from_slice(&j[sos..]);
        assert!(sof < sos);
        assert_eq!(jpeg_info(&late).unwrap().orientation, 1);
        let up = upright_jpeg(&late).unwrap();
        // Not turned, and the late tag, now ahead of the frame header,
        // reads 1 rather than taking effect.
        assert_eq!(jpeg_info(&up).unwrap().orientation, 1);
        assert_eq!(exif_tag(&up), Some(1));
        assert_eq!(
            (
                jpeg_info(&up).unwrap().width,
                jpeg_info(&up).unwrap().height
            ),
            (48, 32)
        );
    }

    #[test]
    fn upright_matches_turning_the_pixels() {
        for &(w, h) in &[(64u32, 48u32), (61, 37), (40, 17), (16, 72)] {
            for o in 1..=8u8 {
                for (src, mw, mh) in sources(w, h, o) {
                    let up = upright_jpeg(&src).expect("upright");
                    let info = jpeg_info(&up).unwrap();
                    assert_eq!(info.orientation, 1);
                    assert_eq!(exif_tag(&up), Some(1), "tag kept and set to 1");
                    // Expected: the source trimmed on mirrored axes, turned.
                    let (fx, fy) = match o {
                        2 | 7 | 8 => (true, matches!(o, 7)),
                        3 => (true, true),
                        4 | 6 => (false, true),
                        _ => (false, false),
                    };
                    let sw = if fx { w - w % mw } else { w };
                    let sh = if fy { h - h % mh } else { h };
                    let d = decode_jpeg(&src).unwrap();
                    let ch = d.channels as usize;
                    let cut = crop(
                        &d.pixels,
                        w,
                        h,
                        ch,
                        crate::CropRect {
                            x: 0,
                            y: 0,
                            width: sw,
                            height: sh,
                        },
                    )
                    .unwrap();
                    let (want, ew, eh) = apply_orientation(&cut, sw, sh, ch, o);
                    assert_eq!((info.width, info.height), (ew, eh), "{w}x{h} o={o}");
                    let got = decode_jpeg(&up).unwrap();
                    assert_eq!(got.pixels.len(), want.len());
                    // Same coefficients, so equal up to IDCT rounding,
                    // except chroma upsampling next to a trimmed edge,
                    // which has become an image edge.
                    for (i, (a, b)) in got.pixels.iter().zip(&want).enumerate() {
                        let (x, y) = ((i / ch) as u32 % ew, (i / ch) as u32 / ew);
                        let edge = x < 2 || y < 2 || x + 2 >= ew || y + 2 >= eh;
                        let diff = a.abs_diff(*b);
                        let max = if edge { 24 } else { 1 };
                        assert!(diff <= max, "{w}x{h} o={o} mcu={mw}x{mh} ({x},{y}): {diff}");
                    }
                }
            }
        }
    }
}
