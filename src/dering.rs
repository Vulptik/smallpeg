//! Clipped-pixel deringing.
//!
//! A decoder clamps every reconstructed sample to 0..=255, so a pixel that
//! sits at the top (or bottom) of the range decodes to the same value for
//! any target at or beyond that end. Those pixels are free variables: the
//! encoder may move them anywhere past the clip point without changing the
//! picture it means to show.
//!
//! This matters at hard edges against white paper (or black ink). The
//! quantization error of an edge block is a ripple that spreads across the
//! whole block; on the flat white side it shows as gray haze around every
//! glyph. If the white side is instead encoded as a smooth continuation of
//! the edge's slope that rises past 255, the ripple rides on top of a
//! signal that is above the clip point and the decoder's clamp erases it.
//!
//! Per block, the clipped pixels are chosen to minimize curvature, the sum
//! of squared second differences along rows and columns:
//!
//! ```text
//! minimize   sum (x[a] - 2 x[b] + x[c])^2   over every row/column triple
//! subject to x_i fixed for unclipped pixels,
//!            x_i in [hi, hi + cap] for pixels clipped high,
//!            x_i in [lo - cap, lo] for pixels clipped low.
//! ```
//!
//! A minimum-curvature fill extends the neighboring slope into the clipped
//! region, which is exactly the overshoot wanted; the cap bounds how far it
//! goes, since a larger amplitude costs bits of its own. The program is
//! solved by projected successive over-relaxation: each clipped pixel in
//! turn moves past its exact one-dimensional optimum by the relaxation
//! factor, then back into its box. The stencil is local, so a sweep costs
//! a dozen operations per clipped pixel, and over-relaxation converges in
//! a handful of sweeps where plain gradient methods need hundreds.

/// Level-shifted thresholds: a luma sample of 254.5 or more decodes to 255,
/// 0.5 or less decodes to 0 (samples are centered on 128 before the DCT).
const HI: f32 = 127.0;
const LO: f32 = -128.0;
const HI_TEST: f32 = 126.5;
const LO_TEST: f32 = -127.5;

/// Stop iterating once no clipped pixel moves by more than this (sample
/// units). Far below what survives quantization at any quality.
const TOL: f32 = 0.05;

/// Over-relaxation factor. Measured on rendered documents: 1.8 reaches in
/// 10 sweeps what plain coordinate descent (1.0) does not in 20; 1.9 is
/// within noise of it.
const OMEGA: f32 = 1.8;

pub(crate) struct Dering {
    cap: f32,
    sweeps: u32,
    low: bool,
}

/// The value of pixel `p` that minimizes the curvature with every other
/// pixel held fixed: the objective is quadratic in `x[p]`, and only the
/// (at most six) row/column triples that contain `p` depend on it.
#[inline]
fn coord_min(x: &[f32; 64], p: usize) -> f32 {
    let (r, c) = (p / 8, p % 8);
    let (mut num, mut den) = (0f32, 0f32);
    // Along one axis: `pos` is p's index on it, `base + j * stride` the
    // pixel at index j. Triples are centered on 1..=6.
    let mut axis = |pos: usize, base: usize, stride: usize| {
        let at = |j: usize| x[base + j * stride];
        if pos >= 2 {
            // p is the right end of the triple centered at pos - 1.
            num += at(pos - 2) - 2.0 * at(pos - 1);
            den += 1.0;
        }
        if (1..=6).contains(&pos) {
            // p is the center.
            num += -2.0 * (at(pos - 1) + at(pos + 1));
            den += 4.0;
        }
        if pos <= 5 {
            // p is the left end of the triple centered at pos + 1.
            num += at(pos + 2) - 2.0 * at(pos + 1);
            den += 1.0;
        }
    };
    axis(c, r * 8, 1);
    axis(r, c, 8);
    -num / den
}

impl Dering {
    /// `cap`: how far past the clip point a pixel may be pushed (sample
    /// units). `sweeps`: solver sweep limit. `low`: also treat pixels
    /// clipped to black (white alone carries nearly all of the gain on
    /// documents, which are dark ink on white paper).
    pub(crate) fn new(cap: f32, sweeps: u32, low: bool) -> Self {
        Self {
            cap: cap.max(0.0),
            sweeps,
            low,
        }
    }

    /// The same settings with pixels clipped at either end treated.
    pub(crate) fn both_ends(self) -> Self {
        Self { low: true, ..self }
    }

    /// Adjust a level-shifted 8x8 block in place. Blocks with no clipped
    /// pixel, or with every pixel clipped to the same end, are untouched.
    pub(crate) fn apply(&self, px: &mut [f32; 64]) {
        let mut idx = [0u8; 64];
        let mut lo_b = [0f32; 64];
        let mut hi_b = [0f32; 64];
        let mut k = 0;
        let (mut n_hi, mut n_lo) = (0, 0);
        for (i, &v) in px.iter().enumerate() {
            if v >= HI_TEST {
                idx[k] = i as u8;
                (lo_b[k], hi_b[k]) = (HI, HI + self.cap);
                k += 1;
                n_hi += 1;
            } else if self.low && v <= LO_TEST {
                idx[k] = i as u8;
                (lo_b[k], hi_b[k]) = (LO - self.cap, LO);
                k += 1;
                n_lo += 1;
            }
        }
        if k == 0 || n_hi == 64 || n_lo == 64 {
            return;
        }
        let idx = &idx[..k];
        for (s, &i) in idx.iter().enumerate() {
            px[i as usize] = px[i as usize].clamp(lo_b[s], hi_b[s]);
        }
        for _ in 0..self.sweeps {
            let mut moved = 0f32;
            for (s, &i) in idx.iter().enumerate() {
                let i = i as usize;
                let target = coord_min(px, i);
                let next = (px[i] + OMEGA * (target - px[i])).clamp(lo_b[s], hi_b[s]);
                moved = moved.max((next - px[i]).abs());
                px[i] = next;
            }
            if moved < TOL {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Curvature of a block (the objective above, unscaled).
    fn curvature(x: &[f32; 64]) -> f32 {
        let mut s = 0f32;
        for r in 0..8 {
            for k in 1..7 {
                let h = x[r * 8 + k - 1] - 2.0 * x[r * 8 + k] + x[r * 8 + k + 1];
                let v = x[(k - 1) * 8 + r] - 2.0 * x[k * 8 + r] + x[(k + 1) * 8 + r];
                s += h * h + v * v;
            }
        }
        s
    }

    #[test]
    fn only_clipped_pixels_move_and_stay_past_the_clip() {
        let d = Dering::new(32.0, 60, true);
        // Left half white paper, right half a dark stroke with a black core.
        let mut px = [0f32; 64];
        for (i, v) in px.iter_mut().enumerate() {
            *v = match i % 8 {
                0..=3 => HI,
                4 => 0.0,
                5 => -100.0,
                _ => LO,
            };
        }
        let before = px;
        d.apply(&mut px);
        for i in 0..64 {
            match i % 8 {
                0..=3 => assert!(px[i] >= HI && px[i] <= HI + 32.0),
                4 | 5 => assert_eq!(px[i], before[i]),
                _ => assert!(px[i] <= LO && px[i] >= LO - 32.0),
            }
        }
        // The stroke's slope carries on past the clip on both sides. (The
        // white pixel right at the edge stays on its bound: the slope
        // extended one step lands below the clip point there.)
        assert!(px[2] > HI + 8.0, "white near the edge overshoots: {px:?}");
        assert!(
            px[6] < LO - 8.0,
            "black next to the edge undershoots: {px:?}"
        );
        assert!(curvature(&px) < curvature(&before));
    }

    #[test]
    fn white_only_by_default_shape() {
        let d = Dering::new(32.0, 60, false);
        let mut px = [0f32; 64];
        for (i, v) in px.iter_mut().enumerate() {
            *v = match i % 8 {
                0..=3 => HI,
                4 => 0.0,
                5 => -100.0,
                _ => LO,
            };
        }
        d.apply(&mut px);
        assert!(px[2] > HI + 8.0);
        assert_eq!(px[6], LO);
        assert_eq!(px[7], LO);
    }

    #[test]
    fn untouched_without_clipping_or_when_flat() {
        let d = Dering::new(32.0, 30, true);
        let mut a = [10f32; 64];
        a[5] = 90.0;
        let a0 = a;
        d.apply(&mut a);
        assert_eq!(a, a0);
        let mut b = [HI; 64];
        d.apply(&mut b);
        assert_eq!(b, [HI; 64]);
    }

    #[test]
    fn coordinate_minimizer_is_the_exact_1d_optimum() {
        let mut x = [0f32; 64];
        for (i, v) in x.iter_mut().enumerate() {
            *v = ((i * 53 + i * i * 7) % 89) as f32 - 44.0;
        }
        for p in [0, 1, 7, 8, 9, 27, 36, 56, 62, 63] {
            let m = coord_min(&x, p);
            let f = |v: f32| {
                let mut y = x;
                y[p] = v;
                curvature(&y)
            };
            // A quadratic's minimum: both neighbors are no lower.
            assert!(
                f(m) <= f(m + 0.1) + 1e-2 && f(m) <= f(m - 0.1) + 1e-2,
                "{p}"
            );
        }
    }
}
