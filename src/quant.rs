//! Base quantization tables, zigzag order and quality scaling.

#[rustfmt::skip]
pub(crate) const BASE_QT_LUMA: [u16; 64] = [
    16, 11, 10, 16, 24, 40, 51, 61,
    12, 12, 14, 19, 26, 58, 60, 55,
    14, 13, 16, 24, 40, 57, 69, 56,
    14, 17, 22, 29, 51, 87, 80, 62,
    18, 22, 37, 56, 68, 109, 103, 77,
    24, 35, 55, 64, 81, 104, 113, 92,
    49, 64, 78, 87, 103, 121, 120, 101,
    72, 92, 95, 98, 112, 100, 103, 99,
];

#[rustfmt::skip]
pub(crate) const BASE_QT_CHROMA: [u16; 64] = [
    17, 18, 24, 47, 99, 99, 99, 99,
    18, 21, 26, 66, 99, 99, 99, 99,
    24, 26, 56, 99, 99, 99, 99, 99,
    47, 66, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
    99, 99, 99, 99, 99, 99, 99, 99,
];

// The Photo preset's base quantization table: one table for both luma and
// chroma. Widely used numeric data — provenance in THIRD_PARTY.md; values
// read from the DQT of a quality-50 encode, where the scaling is exactly 1x.
#[rustfmt::skip]
pub(crate) const BASE_QT_PHOTO: [u16; 64] = [
    16,  16,  16,  18,  25,  37,  56,  85,
    16,  17,  20,  27,  34,  40,  53,  75,
    16,  20,  24,  31,  43,  62,  91, 135,
    18,  27,  31,  40,  53,  74, 106, 156,
    25,  34,  43,  53,  69,  94, 131, 189,
    37,  40,  62,  74,  94, 124, 169, 238,
    56,  53,  91, 106, 131, 169, 226, 255,
    85,  75, 135, 156, 189, 238, 255, 255,
];

#[rustfmt::skip]
pub(crate) const ZIGZAG: [usize; 64] = [
     0,  1,  8, 16,  9,  2,  3, 10,
    17, 24, 32, 25, 18, 11,  4,  5,
    12, 19, 26, 33, 40, 48, 41, 34,
    27, 20, 13,  6,  7, 14, 21, 28,
    35, 42, 49, 56, 57, 50, 43, 36,
    29, 22, 15, 23, 30, 37, 44, 51,
    58, 59, 52, 45, 38, 31, 39, 46,
    53, 60, 61, 54, 47, 55, 62, 63,
];

/// Conventional quality scaling of a base table (quality 50 = the table).
pub(crate) fn scaled_quant_table(base: &[u16; 64], quality: u8) -> [u16; 64] {
    let scale = if quality < 50 {
        5000 / quality as u32
    } else {
        200 - 2 * quality as u32
    };
    let mut out = [0u16; 64];
    for (o, &b) in out.iter_mut().zip(base) {
        *o = ((b as u32 * scale + 50) / 100).clamp(1, 255) as u16;
    }
    out
}
