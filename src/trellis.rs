//! Rate-distortion trellis quantization of AC coefficients.

use crate::symbols::category;

/// Estimated code length in bits for every symbol, from pass-1 frequencies.
/// Entropy with add-half smoothing, so symbols the trellis wants to try but
/// pass 1 never produced still get a finite (pessimistic) price.
pub(crate) fn estimate_bits(freq: &[u32; 256]) -> [f32; 256] {
    let total: u64 = freq.iter().map(|&f| f as u64).sum();
    let denom = total as f32 + 128.0;
    let mut out = [0f32; 256];
    for (o, &f) in out.iter_mut().zip(freq) {
        *o = (-((f as f32 + 0.5) / denom).log2()).clamp(1.0, 16.0);
    }
    out
}

/// Rate-distortion optimal AC quantization for one block (trellis
/// quantization, implemented from the standard R-D formulation).
///
/// Dynamic programming over "position of the last nonzero coefficient":
/// node k holds the cheapest encoding of AC 1..k that ends with a nonzero
/// at k. An edge j -> k zeroes everything in between and prices the real
/// run-length structure (ZRL for runs > 15, then the (run,size) symbol plus
/// amplitude bits). Candidate magnitudes at k are round(|t|) and one less —
/// anything further from t is worse in both distortion and rate. The final
/// choice adds trailing-zero distortion and the EOB symbol where required.
///
/// Distortion is measured in quantization-step units ((t - v)^2) — the quant
/// table's own frequency weighting. Weighting by true squared pixel error
/// (q^2) was tried and is much worse: it makes small low-frequency
/// coefficients 100x cheaper to drop than the table intends, and smooth
/// gradients fall apart. `lambda` converts bits into step-unit currency.
/// `wt` (zigzag order) scales each coefficient's squared error; all ones
/// is plain step units, see `TRELLIS_FREQ_GAMMA_PHOTO` for the mild
/// frequency weighting the Photo preset uses.
pub(crate) fn trellis_ac(
    raw: &[f32; 64],
    out: &mut [i16; 64],
    ac_bits: &[f32; 256],
    lambda: f32,
    wt: &[f32; 64],
) {
    // zsum[k] = distortion of zeroing AC coefficients 1..=k.
    let mut zsum = [0f32; 64];
    let mut acc = 0f32;
    for k in 1..64 {
        acc += raw[k] * raw[k] * wt[k];
        zsum[k] = acc;
    }

    let mut best = [f32::INFINITY; 64]; // best[k]: cost with last nonzero at k
    let mut from = [0usize; 64];
    let mut mag = [0i16; 64];
    // Positions that can host a nonzero (rounded magnitude >= 1). Only these
    // — plus the virtual start 0 — ever have finite best[], so the DP walks
    // node-to-node instead of scanning every index in between. Sparse blocks
    // (the common case) get quadratically cheaper.
    let mut nodes = [0usize; 63];
    let mut nn = 0usize;
    for k in 1..64 {
        let t = raw[k].abs();
        let r = t.round() as i32;
        if r == 0 {
            continue;
        }
        // Candidate magnitudes with their (size, distortion).
        let mut cands = [(0i32, 0u8, 0f32); 2];
        let mut nc = 0;
        let mut dmin = f32::INFINITY;
        for a in [r, r - 1] {
            if a >= 1 {
                let d = (t - a as f32) * (t - a as f32) * wt[k];
                cands[nc] = (a, category(a), d);
                nc += 1;
                dmin = dmin.min(d);
            }
        }
        // Walk predecessor nodes nearest-first; the zero-run distortion
        // grows monotonically as j retreats, so once it alone rules out an
        // improvement, no farther j can win either.
        for &j in nodes[..nn].iter().rev().chain(std::iter::once(&0)) {
            let zero_d = zsum[k - 1] - zsum[j];
            if zero_d + dmin >= best[k] {
                break;
            }
            let base = if j == 0 { 0.0 } else { best[j] };
            let mut run = k - j - 1;
            let mut zrl = 0f32;
            while run > 15 {
                zrl += ac_bits[0xF0];
                run -= 16;
            }
            for &(a, s, d) in &cands[..nc] {
                let rate = zrl + ac_bits[(run << 4) | s as usize] + s as f32;
                let cost = base + zero_d + d + lambda * rate;
                if cost < best[k] {
                    best[k] = cost;
                    from[k] = j;
                    mag[k] = a as i16;
                }
            }
        }
        nodes[nn] = k;
        nn += 1;
    }

    // Close the block: pay for trailing zeros, and EOB unless we end at 63.
    let eob = lambda * ac_bits[0x00];
    let mut end = 0usize;
    let mut end_cost = zsum[63] + eob; // all-zero AC
    for &j in &nodes[..nn] {
        let c = best[j] + (zsum[63] - zsum[j]) + if j < 63 { eob } else { 0.0 };
        if c < end_cost {
            end_cost = c;
            end = j;
        }
    }

    out[1..].fill(0);
    let mut k = end;
    while k > 0 {
        out[k] = if raw[k] < 0.0 { -mag[k] } else { mag[k] };
        k = from[k];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trellis_weight_protects_coefficient() {
        // A lone 0.6-step coefficient: dropping costs 0.36, keeping costs
        // 0.16 plus 7 bits at lambda 0.06 (0.42) — dropped in plain step
        // units, kept once its error counts four-fold.
        let mut raw = [0f32; 64];
        raw[5] = 0.6;
        let bits = [6f32; 256];
        let out = [0i16; 64];
        let mut plain = out;
        trellis_ac(&raw, &mut plain, &bits, 0.06, &[1.0; 64]);
        assert!(plain.iter().all(|&v| v == 0));
        let mut wt = [1f32; 64];
        wt[5] = 4.0;
        let mut weighted = out;
        trellis_ac(&raw, &mut weighted, &bits, 0.06, &wt);
        assert_eq!(weighted.iter().filter(|&&v| v != 0).count(), 1);
    }
}
