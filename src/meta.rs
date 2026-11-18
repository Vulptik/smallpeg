//! JPEG metadata: the EXIF orientation tag, stripping metadata segments
//! (clean-room from the EXIF 2.3 / TIFF 6.0 layout and ITU T.81 markers),
//! and applying an orientation to decoded pixels.

use crate::optimize::next_marker;

/// The EXIF Orientation tag (0x0112) from an APP1 payload
/// (`Exif\0\0` + TIFF stream), if present and in 1..=8.
pub(crate) fn exif_orientation(app1: &[u8]) -> Option<u8> {
    exif_orientation_at(app1).map(|(o, _, _)| o)
}

/// [`exif_orientation`] with where its value sits: the offset of the
/// two-byte SHORT in `app1`, and whether the TIFF stream is little-endian.
pub(crate) fn exif_orientation_at(app1: &[u8]) -> Option<(u8, usize, bool)> {
    let t = app1.strip_prefix(b"Exif\0\0")?;
    let le = match t.get(..4)? {
        [b'I', b'I', 42, 0] => true,
        [b'M', b'M', 0, 42] => false,
        _ => return None,
    };
    let u16_at = |p: usize| -> Option<u16> {
        let b = t.get(p..p.checked_add(2)?)?;
        Some(if le {
            u16::from_le_bytes([b[0], b[1]])
        } else {
            u16::from_be_bytes([b[0], b[1]])
        })
    };
    let u32_at = |p: usize| -> Option<u32> {
        let b = t.get(p..p.checked_add(4)?)?;
        let b = [b[0], b[1], b[2], b[3]];
        Some(if le {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    };
    // IFD0: entry count, then 12-byte entries (tag, type, count, value).
    let ifd = u32_at(4)? as usize;
    let n = u16_at(ifd)? as usize;
    for i in 0..n {
        let e = ifd.checked_add(2 + 12 * i)?;
        if u16_at(e)? == 0x0112 {
            // SHORT, count 1: the value sits in the first two value bytes.
            if u16_at(e + 2)? != 3 || u32_at(e + 4)? != 1 {
                return None;
            }
            let v = u16_at(e + 8)?;
            return (1..=8).contains(&v).then_some((v as u8, 6 + e + 8, le));
        }
    }
    None
}

/// A minimal big-endian TIFF stream holding only an Orientation tag.
fn orientation_tiff(o: u8) -> [u8; 26] {
    [
        b'M', b'M', 0, 42, 0, 0, 0, 8, // header, IFD0 at 8
        0, 1, // one entry
        0x01, 0x12, 0, 3, 0, 0, 0, 1, 0, o, 0, 0, // Orientation, SHORT, 1
        0, 0, 0, 0, // no next IFD
    ]
}

/// End of the entropy-coded data starting at `p`: the next marker that is
/// not a stuffed byte, a restart marker or fill.
fn entropy_end(d: &[u8], mut p: usize) -> usize {
    while p + 1 < d.len() {
        if d[p] == 0xFF {
            match d[p + 1] {
                0x00 | 0xD0..=0xD7 => p += 2,
                0xFF => p += 1,
                _ => return p,
            }
        } else {
            p += 1;
        }
    }
    d.len()
}

/// Remove metadata from a JPEG without touching the image data.
///
/// Dropped: EXIF (GPS position, camera, dates, thumbnails), XMP, IPTC and
/// every other APPn segment, comments, and anything after the end-of-image
/// marker (such as the extra images some phones append). Kept: what
/// decoders use to reproduce the pixels and colors — the JFIF header, an
/// ICC profile, the Adobe APP14 color-transform segment, and all tables
/// and scans byte-for-byte. If the EXIF data carried an orientation other
/// than 1, a minimal EXIF segment with only that tag (26 bytes) takes its
/// place, so photos keep displaying upright.
///
/// Lossless and cheap (no decoding): the result decodes to exactly the
/// same pixels. Composes with the optimizer:
///
/// ```
/// # let rgb = vec![128u8; 16 * 16 * 3];
/// # let src = smallpeg::Encoder::new(80).encode_rgb(&rgb, 16, 16);
/// let clean = smallpeg::strip_metadata(&src).expect("a JPEG");
/// let small = smallpeg::optimize_jpeg(&clean);
/// # assert!(small.is_some());
/// ```
///
/// Returns `None` for input that is not a well-formed marker sequence
/// from SOI to EOI. Takes untrusted input and never panics.
pub fn strip_metadata(jpeg: &[u8]) -> Option<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return None;
    }
    let mut out = Vec::with_capacity(jpeg.len());
    out.extend_from_slice(&[0xFF, 0xD8]);
    let mut orientation_done = false;
    let mut pos = 2usize;
    loop {
        let (marker, after) = next_marker(jpeg, pos)?;
        match marker {
            0xD9 => {
                out.extend_from_slice(&[0xFF, 0xD9]);
                return Some(out);
            }
            0xD8 => return None,
            0x01 | 0xD0..=0xD7 => {
                out.extend_from_slice(&jpeg[after - 2..after]);
                pos = after;
                continue;
            }
            _ => {}
        }
        if after + 2 > jpeg.len() {
            return None;
        }
        let seg_len = u16::from_be_bytes([jpeg[after], jpeg[after + 1]]) as usize;
        if seg_len < 2 || after + seg_len > jpeg.len() {
            return None;
        }
        let payload = &jpeg[after + 2..after + seg_len];
        let seg = &jpeg[after - 2..after + seg_len];
        pos = after + seg_len;
        let keep = match marker {
            0xE0 => payload.starts_with(b"JFIF\0"),
            0xE1 => {
                // The first EXIF orientation is rewritten in place.
                if !orientation_done && let Some(o) = exif_orientation(payload) {
                    orientation_done = true;
                    if o != 1 {
                        let tiff = orientation_tiff(o);
                        out.extend_from_slice(&[0xFF, 0xE1]);
                        out.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
                        out.extend_from_slice(b"Exif\0\0");
                        out.extend_from_slice(&tiff);
                    }
                }
                false
            }
            0xE2 => payload.starts_with(b"ICC_PROFILE\0"),
            0xEE => payload.starts_with(b"Adobe"),
            0xE3..=0xEF | 0xFE => false,
            _ => true,
        };
        if keep {
            out.extend_from_slice(seg);
        }
        if marker == 0xDA {
            let end = entropy_end(jpeg, pos);
            out.extend_from_slice(&jpeg[pos..end]);
            pos = end;
        }
    }
}

/// Apply an EXIF orientation (1..=8, as returned in
/// [`JpegInfo::orientation`](crate::JpegInfo::orientation)) to decoded
/// pixels, so they are stored the way the image is meant to be viewed.
/// Returns the new pixels with the new width and height (swapped for
/// orientations 5–8, the 90° turns).
///
/// Decoders return pixels as stored; phone cameras store the sensor's
/// landscape frame and record the turn in EXIF. Apply this before
/// re-encoding pixels without that EXIF, or the result shows sideways.
/// (Do not apply it to images placed in a PDF: PDF viewers ignore EXIF
/// and draw the stored pixels.) Values outside 1..=8 are treated as 1.
///
/// Works for any interleaved 8-bit layout: `channels` is 1 for gray, 3
/// for RGB, 4 for CMYK.
///
/// # Panics
///
/// Panics if `pixels.len()` is not exactly `width * height * channels`,
/// or if `channels` is 0.
pub fn apply_orientation(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: usize,
    orientation: u8,
) -> (Vec<u8>, u32, u32) {
    assert!(channels > 0);
    let (w, h) = (width as usize, height as usize);
    assert_eq!(pixels.len(), w * h * channels);
    let o = if (1..=8).contains(&orientation) {
        orientation
    } else {
        1
    };
    let (ow, oh) = if o >= 5 { (h, w) } else { (w, h) };
    if o == 1 || pixels.is_empty() {
        return (pixels.to_vec(), ow as u32, oh as u32);
    }
    // Source pixel of output (x, y); linear in x and y, so it reduces to a
    // start index and two signed steps.
    let (wi, hi) = (w as isize, h as isize);
    let src = |x: isize, y: isize| -> isize {
        let (sx, sy) = match o {
            2 => (wi - 1 - x, y),
            3 => (wi - 1 - x, hi - 1 - y),
            4 => (x, hi - 1 - y),
            5 => (y, x),
            6 => (y, hi - 1 - x),
            7 => (wi - 1 - y, hi - 1 - x),
            _ => (wi - 1 - y, x), // 8
        };
        sy * wi + sx
    };
    let start = src(0, 0);
    let (sx, sy) = (src(1, 0) - start, src(0, 1) - start);
    let mut out = vec![0u8; pixels.len()];
    let c = channels;
    for (y, row) in out.chunks_exact_mut(ow * c).enumerate() {
        let mut s = start + sy * y as isize;
        for px in row.chunks_exact_mut(c) {
            let i = s as usize * c;
            px.copy_from_slice(&pixels[i..i + c]);
            s += sx;
        }
    }
    (out, ow as u32, oh as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Encoder, decode_jpeg, jpeg_info, optimize_jpeg};

    /// Little-endian TIFF: Make "abc" then Orientation `o`.
    fn tiff_le(o: u16) -> Vec<u8> {
        let mut t = b"II*\0\x08\0\0\0".to_vec();
        t.extend_from_slice(&2u16.to_le_bytes());
        t.extend_from_slice(&[0x0F, 0x01, 2, 0, 4, 0, 0, 0, b'a', b'b', b'c', 0]);
        t.extend_from_slice(&[0x12, 0x01, 3, 0, 1, 0, 0, 0]);
        t.extend_from_slice(&o.to_le_bytes());
        t.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        t
    }

    fn app1(tiff: &[u8]) -> Vec<u8> {
        let mut p = b"Exif\0\0".to_vec();
        p.extend_from_slice(tiff);
        p
    }

    fn segments(j: &[u8]) -> Vec<(u8, Vec<u8>)> {
        let mut out = Vec::new();
        let mut pos = 2;
        while j[pos + 1] != 0xDA {
            let m = j[pos + 1];
            let len = ((j[pos + 2] as usize) << 8) | j[pos + 3] as usize;
            out.push((m, j[pos + 4..pos + 2 + len].to_vec()));
            pos += 2 + len;
        }
        out
    }

    fn insert_before_dqt(j: &[u8], marker: u8, payload: &[u8]) -> Vec<u8> {
        let at = j.windows(2).position(|w| w == [0xFF, 0xDB]).unwrap();
        let mut o = j[..at].to_vec();
        o.extend_from_slice(&[0xFF, marker]);
        o.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        o.extend_from_slice(payload);
        o.extend_from_slice(&j[at..]);
        o
    }

    fn test_rgb(w: u32, h: u32) -> Vec<u8> {
        (0..w * h * 3).map(|i| (i * 37 % 251) as u8).collect()
    }

    #[test]
    fn orientation_parses_both_byte_orders() {
        for o in 1..=8u16 {
            assert_eq!(exif_orientation(&app1(&tiff_le(o))), Some(o as u8));
            let be = orientation_tiff(o as u8);
            assert_eq!(exif_orientation(&app1(&be)), Some(o as u8));
            // Agrees with an independent reader.
            let theirs = image::metadata::Orientation::from_exif_chunk(&be).unwrap();
            assert_eq!(theirs.to_exif(), o as u8);
        }
        assert_eq!(exif_orientation(&app1(&tiff_le(0))), None);
        assert_eq!(exif_orientation(&app1(&tiff_le(9))), None);
        assert_eq!(exif_orientation(b"Exif\0\0MM\0*"), None);
        // Truncated anywhere: None, never a panic.
        let full = app1(&tiff_le(6));
        for n in 0..full.len() {
            let _ = exif_orientation(&full[..n]);
        }
    }

    #[test]
    fn jpeg_info_reports_orientation() {
        let rgb = test_rgb(24, 16);
        let j = Encoder::new(80).exif(&tiff_le(6)).encode_rgb(&rgb, 24, 16);
        assert_eq!(jpeg_info(&j).unwrap().orientation, 6);
        let plain = Encoder::new(80).encode_rgb(&rgb, 24, 16);
        assert_eq!(jpeg_info(&plain).unwrap().orientation, 1);
    }

    #[test]
    fn apply_orientation_matches_image_crate() {
        let (w, h) = (5u32, 3u32);
        let rgb = test_rgb(w, h);
        for o in 1..=8u8 {
            let (ours, ow, oh) = apply_orientation(&rgb, w, h, 3, o);
            let mut img = image::DynamicImage::ImageRgb8(
                image::RgbImage::from_raw(w, h, rgb.clone()).unwrap(),
            );
            img.apply_orientation(image::metadata::Orientation::from_exif(o).unwrap());
            assert_eq!((ow, oh), (img.width(), img.height()), "o={o}");
            assert_eq!(ours, img.to_rgb8().into_raw(), "o={o}");
        }
        // Inverse pairs: 6 then 8 is identity; 5 and 7 undo themselves.
        let (a, aw, ah) = apply_orientation(&rgb, w, h, 3, 6);
        assert_eq!(apply_orientation(&a, aw, ah, 3, 8).0, rgb);
        for o in [2u8, 3, 4, 5, 7] {
            let (a, aw, ah) = apply_orientation(&rgb, w, h, 3, o);
            assert_eq!(apply_orientation(&a, aw, ah, 3, o).0, rgb, "o={o}");
        }
        // Single row / column and four channels.
        let line: Vec<u8> = (0..4 * 7).map(|i| i as u8).collect();
        let (t, tw, th) = apply_orientation(&line, 7, 1, 4, 6);
        assert_eq!((tw, th), (1, 7));
        assert_eq!(apply_orientation(&t, tw, th, 4, 8).0, line);
        assert_eq!(apply_orientation(&rgb, w, h, 3, 0).0, rgb);
    }

    #[test]
    fn strip_keeps_pixels_color_and_orientation_only() {
        let (w, h) = (40u32, 30u32);
        let rgb = test_rgb(w, h);
        let icc: Vec<u8> = (0..300u32).map(|i| i as u8).collect();
        let j = Encoder::new(80)
            .exif(&tiff_le(6))
            .icc_profile(&icc)
            .encode_rgb(&rgb, w, h);
        let j = insert_before_dqt(&j, 0xFE, b"a comment");
        let j = insert_before_dqt(&j, 0xE1, b"http://ns.adobe.com/xap/1.0/\0<x/>");
        let j = insert_before_dqt(&j, 0xED, b"Photoshop 3.0\0iptc");
        let mut j = insert_before_dqt(&j, 0xEE, b"Adobe\0\x64\0\0\0\0\x01");
        j.extend_from_slice(b"\xFF\xD8appended second image\xFF\xD9");

        let s = strip_metadata(&j).unwrap();
        let segs = segments(&s);
        let kinds: Vec<u8> = segs.iter().map(|(m, _)| *m).collect();
        assert!(!kinds.contains(&0xFE) && !kinds.contains(&0xED));
        let app1: Vec<&Vec<u8>> = segs
            .iter()
            .filter(|(m, _)| *m == 0xE1)
            .map(|(_, p)| p)
            .collect();
        assert_eq!(app1.len(), 1, "only the minimal orientation EXIF");
        assert_eq!(app1[0].len(), 6 + 26);
        assert!(
            segs.iter()
                .any(|(m, p)| *m == 0xE2 && p.starts_with(b"ICC_PROFILE\0"))
        );
        assert!(
            segs.iter()
                .any(|(m, p)| *m == 0xEE && p.starts_with(b"Adobe"))
        );
        assert!(
            segs.iter()
                .any(|(m, p)| *m == 0xE0 && p.starts_with(b"JFIF\0"))
        );
        assert!(s.ends_with(&[0xFF, 0xD9]) && !s.windows(8).any(|w| w == b"appended"));
        assert_eq!(jpeg_info(&s).unwrap().orientation, 6);
        assert_eq!(decode_jpeg(&s).unwrap(), decode_jpeg(&j).unwrap());
        assert!(optimize_jpeg(&s).is_some());

        // Orientation 1 (or none): no EXIF at all afterwards.
        let k = Encoder::new(80).exif(&tiff_le(1)).encode_rgb(&rgb, w, h);
        let s = strip_metadata(&k).unwrap();
        assert!(!segments(&s).iter().any(|(m, _)| *m == 0xE1));

        // Progressive (several scans) and restart markers survive intact.
        let p = Encoder::new(80)
            .progressive(true)
            .encode_rgb(&test_rgb(160, 120), 160, 120);
        let s = strip_metadata(&p).unwrap();
        assert_eq!(decode_jpeg(&s).unwrap(), decode_jpeg(&p).unwrap());
    }

    #[test]
    fn strip_hostile_input_never_panics() {
        let rgb = test_rgb(24, 16);
        let j = Encoder::new(80)
            .exif(&tiff_le(3))
            .progressive(true)
            .encode_rgb(&rgb, 24, 16);
        for n in 0..j.len() {
            let _ = strip_metadata(&j[..n]);
        }
        let mut seed = 99u64;
        for _ in 0..3000 {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let i = (seed >> 33) as usize % j.len();
            let mut m = j.clone();
            m[i] ^= (seed >> 17) as u8 | 1;
            let _ = strip_metadata(&m);
            let _ = jpeg_info(&m);
        }
        assert!(strip_metadata(b"").is_none());
        assert!(strip_metadata(b"\xff\xd8\xff\xd9").is_some());
    }
}
