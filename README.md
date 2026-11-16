# smallpeg

A from-scratch, dependency-free JPEG encoder in pure Rust — color transform,
DCT, quantization, Huffman coding, all hand-rolled from the JPEG standard
(ITU-T T.81). It aims at a gap: a modern, perceptually measured JPEG
encoder under a permissive license, with zero dependencies and first-class
`wasm32-unknown-unknown` support.

The name is what it does: small JPEGs.

## Status: v0.13

- Baseline sequential and progressive JFIF, grayscale + RGB, 4:4:4 / 4:2:0
- **Progressive by default, chosen by measurement**: spectral selection
  plus successive approximation, with the scan script optimized per image
  — the encoder prices every candidate (band split points, point-transform
  depth, DC precision) with real statistics passes and keeps the cheapest,
  then renders both progressive and baseline entropy streams and ships the
  smaller (the DCT/trellis work is shared, so this is cheap). No fixed
  script fits both smooth gradients and noisy photos
- **Quality-adaptive table shape**: at high quality the luma table's
  high-frequency steps are geometrically flattened (ramping in above
  q60) — this wins on textured and document content under both PSNR and
  SSIMULACRA2. Gradient-dominant images are exempt, classified per image
  from the quant-independent DCT energy split plus nonzero density.
  The obvious alternative — masking-based per-block deadzone — measured
  strictly worse under SSIMULACRA2 (SSIM-family metrics already model
  masking; dropping more in texture attacks exactly what they score)
- **Quantization table presets**: `QuantPreset::Photo` (default — one
  fine-grained table shared by luma and chroma, noticeably better on
  smooth gradients and at low quality; the table itself is widely used
  numeric data, see THIRD_PARTY.md) and `QuantPreset::AnnexK` (the
  classic T.81 example tables — still the better pick for documents,
  scans, and noisy content at high quality). The chroma trellis strength
  follows the preset automatically. At 4:4:4, where chroma has four times
  the samples, `Photo` doubles its chroma steps (fading out above q95):
  5–10% smaller at equal SSIMULACRA2 on held-out photos
- **Per-image optimized Huffman tables**, always on — the encoder is
  two-pass by design
- **DC trellis**: the DC coefficients form per-component DPCM chains, so
  optimal rounding is a Viterbi pass over {round−1, round, round+1} per
  block. Worth another 1–2.5% on smooth/document content at equal
  quality (PSNR and SSIMULACRA2 both), neutral on photos
- **Clipped-pixel deringing**, on by default: a decoder clamps samples to
  0–255, so luma pixels at full white decode the same whatever value past
  255 they are encoded as. In blocks that mix white with darker content
  those pixels are re-chosen per block as the minimum-curvature
  continuation of the neighboring edge (a small box-constrained problem
  solved by projected over-relaxation), so quantization ripple lands
  above 255 and is clamped away instead of showing as haze around text
  and line art. On rendered documents (Latin and Japanese text, tables,
  charts) this is 17–19% smaller at equal SSIMULACRA2 and 10–13% at equal
  butteraugli; photographs are neutral. Costs ~40% more encode time on
  text-heavy pages, nothing measurable on photos. Off whenever a
  quantization step is 1 (very high quality), where overshoot could push
  coefficients past what baseline JPEG can code. Neutral on images that
  are themselves decoded JPEGs (re-compression), where
  `Encoder::deringing(false)` saves the ~25% encode time
- **Trellis quantization by default**: every AC coefficient is
  re-decided by dynamic programming against the real run-length/EOB
  structure, pricing bits with rate estimates from pass 1 and distortion
  in quantization-step units. With the `AnnexK` tables chroma runs at a
  much smaller lambda — its coarse quant steps make step-unit distortion
  undervalue real pixel error, and full-strength chroma trellis traded
  ~2 dB for ~2% on saturated photos. With the `Photo` tables (RGB input)
  the distortion is mildly frequency-weighted and chroma runs at the
  luma lambda: 0–2% smaller at 4:2:0 and 2–3.5% at 4:4:4 at equal
  SSIMULACRA2 / butteraugli on held-out images
- **AAN scaled DCT** (Arai–Agui–Nakajima, 5 multiplies per 8-point pass)
  with descaling folded into the quantization divisors, verified
  bit-for-bit against the naive orthonormal transform in tests
- **Lossless JPEG re-optimization** (`optimize_jpeg`): an existing
  baseline or progressive JPEG is entropy-decoded back to its quantized
  coefficients (Huffman layer only — no IDCT, no color conversion,
  deliberately not a full decoder) and re-fed through the
  optimized-Huffman baseline and scan-optimized progressive emitters
  above; the smaller rendering wins. Pixels are bit-identical:
  coefficients, quantization tables, sampling and APPn/COM metadata
  (EXIF, ICC) are preserved. Typical savings on camera-grade input run
  3–10% (up to ~20% on smooth content); already-optimized files
  round-trip at ±0.2%. Hostile input is handled by bounds-checked
  parsing that returns `None`, verified by truncation and bit-flip
  sweeps; the one known refusal is files whose restart-marker cadence
  contradicts their own DRI header — mainstream decoders treat those
  as corrupt data and resynchronize. `optimize_jpeg_baseline` restricts
  the output to baseline sequential (converting progressive inputs
  losslessly) for progressive-averse targets like PDF embedding — both
  are stable API
- **ICC / EXIF embedding** (`Encoder::icc_profile` / `Encoder::exif`,
  stable API): attach a color profile (chunked APP2 per the ICC spec
  when over 64 KB) and EXIF metadata to freshly encoded output —
  non-sRGB images survive re-encoding with their color intent intact
  when the caller passes the profile along (see
  [Color profiles](#color-profiles)). `optimize_jpeg` preserves the same
  segments on the lossless path
- **Metadata removal and orientation** (`strip_metadata`,
  `JpegInfo::orientation`, `apply_orientation`): drop EXIF/GPS, XMP,
  comments and appended data losslessly while keeping color (ICC, Adobe)
  and the photo's orientation; or read the orientation and turn decoded
  pixels upright before re-encoding (phone photos store a sideways frame
  plus an EXIF turn)
- **CMYK without color conversion** (`decode_cmyk` / `resize_cmyk` /
  `Encoder::encode_cmyk`): four-component JPEGs (plain CMYK and Adobe
  YCCK) are decoded to their stored channel values, resized per channel
  and re-encoded, with no trip through RGB. The encoder writes its own
  Adobe APP14 whose transform byte always matches the coding it
  performed, and the inversion convention of Adobe files is never
  interpreted, so it survives as is. Verified against an independent
  decoder on a print-oriented PDF's CMYK images (within ±2); re-encoding
  them at q85
  cut 42% at full resolution. Measured on those images: YCCK is 23–60%
  smaller than plain CMYK at equal quality, and the `Photo` preset 3–11%
  smaller than `AnnexK`
- **Decoding** (`decode_jpeg`): grayscale and color JPEGs, baseline and
  progressive, any sampling, to 8-bit gray or RGB — color space decided
  by the conventional JFIF / Adobe / component-id rules, chroma upsampled
  by the conventional 3/4–1/4 interpolation. Within ±2 of an independent
  decoder on 366 of 373 JPEGs from real PDFs (the rest are 1–2 pixel
  wide strips, where edge handling differs). Untrusted input returns
  `None`, never panics.
  `jpeg_info` reads the frame header without decoding, and `Limits`
  (`decode_jpeg_with` etc.) caps pixels and scans before anything is
  allocated (defaults: 100 megapixels, 128 scans)
- **Lanczos3 resampling** (`resize_rgb` / `resize_gray`) — for photos,
  downsampling decides more of the size/quality trade than the encoder
  does, so a "make this image small" tool needs both halves. Verified
  against a reference implementation. Decoupled from the encoder API.
  `resize_rgb_linear` / `resize_gray_linear` filter in linear light
  instead of on the stored sRGB codes, which keeps fine light-and-dark
  detail from darkening when reduced; `fit_within` computes the
  aspect-preserving target size for a bounding box, and `crop` cuts a
  rectangle out of pixels with any channel count
- Zero dependencies, pure Rust, compiles to `wasm32-unknown-unknown` as-is.
  An optional `parallel` cargo feature (std threads only, off by default,
  not for wasm) runs the trellis across cores with byte-identical output

Every decision above was made by measurement: equal-quality file size
interpolated from q50–95 rate–distortion sweeps, scored with PSNR,
SSIMULACRA2 and butteraugli, decoded with independent decoders, across a
corpus of documents, scans, smooth gradients and photographs. At equal
quality the output is routinely substantially smaller than
non-perceptual encoders produce, and competitive with the strongest
perceptually tuned ones; the one known remaining gap is saturated
photographs at low quality, where full psychovisual adaptive
quantization keeps a few percent of edge.

Speed: A4 300 dpi RGB scan in ~200 ms native — the progressive scan
search and emission walk a CSR sparse view of the coefficients (only
nonzeros are touched, and one pass per band prices all three point
transforms), which took the progressive stage from dominant to cheap.
Peak memory is ~10 bytes/pixel: color conversion and DCT are streamed
one MCU row at a time (no full-image pixel planes), and the pre-rounding
coefficients are freed once the trellis outcome is final.

## Usage

```rust
let jpeg: Vec<u8> = smallpeg::Encoder::new(85)
    .sampling(smallpeg::Sampling::S420)
    .encode_rgb(&rgb_bytes, width, height);

// Downsample first when the target is "small" (usually it is):
let half = smallpeg::resize_rgb(&rgb_bytes, width, height, width / 2, height / 2);
let jpeg = smallpeg::Encoder::new(80).encode_rgb(&half, width / 2, height / 2);

// Crop, then reduce to fit 1600x1600 keeping the aspect ratio:
let rect = smallpeg::CropRect { x: 100, y: 50, width: 3000, height: 2000 };
if let Some(cut) = smallpeg::crop(&rgb_bytes, width, height, 3, rect) {
    let (w, h) = smallpeg::fit_within(rect.width, rect.height, 1600, 1600);
    let small = smallpeg::resize_rgb_linear(&cut, rect.width, rect.height, w, h);
    let jpeg = smallpeg::Encoder::new(80).encode_rgb(&small, w, h);
}

// Shrink an existing JPEG without touching its pixels:
if let Some(opt) = smallpeg::optimize_jpeg(&jpeg_bytes) {
    if opt.len() < jpeg_bytes.len() {
        // same pixels, fewer bytes
    }
}
// optimize_jpeg_baseline() is the same but always emits baseline
// sequential output (e.g. for PDF embedding); a progressive input is
// converted losslessly.

// CMYK / YCCK JPEGs: no RGB round trip, Adobe APP14 coding preserved.
if let Some(img) = smallpeg::decode_cmyk(&cmyk_jpeg) {
    let (w, h) = (img.width / 2, img.height / 2);
    let small = smallpeg::resize_cmyk(&img.pixels, img.width, img.height, w, h);
    let jpeg = smallpeg::Encoder::new(85).encode_cmyk(&small, w, h, img.adobe);
}
```

### Color profiles

The encoder takes raw RGB bytes and never sees the source file, so it
cannot know what color space those bytes are in. Output carries no ICC
profile unless you attach one, and decoders then read it as sRGB. If the
source is in another space (Display P3 is the usual case: iPhone photos,
macOS screenshots and renders), colors shift visibly unless you forward
the source's profile:

```rust
let mut enc = smallpeg::Encoder::new(85);
if let Some(icc) = &source_icc_profile {
    // e.g. the profile your image decoder read from the source file
    enc = enc.icc_profile(icc);
}
let jpeg = enc.encode_rgb(&rgb_bytes, width, height);
```

Alternatively, convert the pixels to sRGB before encoding and omit the
profile. Most image decoding crates hand back raw pixels and leave the
profile aside, so it is easy to drop by accident. `optimize_jpeg`
is unaffected, because it keeps the input's APPn segments as they are.

## Roadmap

1. ~~**Trellis quantization**~~ — done (v0.2), on by default.
2. ~~Fast DCT (AAN)~~ — done (v0.3), plus a node-walking trellis DP.
3. ~~Lanczos3 resize~~ — done (v0.4).
4. ~~Progressive mode~~ — done (v0.5), with per-image scan optimization
   and a baseline fallback chosen by measured size.
5. ~~Quantization table presets~~ — done (v0.6); this, not deadzone, was
   most of the smooth/saturated gap.
6. ~~Per-image trellis lambda~~ — done (v0.6.1) for gradient-like content
   (AC energy concentrated in the first zigzag positions): a 2x lambda is
   tried and kept when the true pixel-domain error stays within ~0.3 dB.
7. ~~Adaptive quantization~~ — resolved differently (v0.8): masking-based
   deadzone measured strictly worse under SSIM-family metrics; the real
   win was the quality-adaptive table shape. Remaining known gap:
   saturated photos below ~q75. Every portable lever was measured
   against it and failed — table-range expansion, linear-light chroma
   averaging, edge-adjacency lambda fields (one- and two-sided), and
   decoder-model-aware "sharp" chroma downsampling (sharpened chroma
   costs coefficients the low-quality table immediately re-quantizes
   away). The residue is a full psychovisual adaptive-quantization
   field, which resists piecewise distillation.
7b. ~~DC trellis~~ — done (v0.9).
8. ~~Streaming (MCU-row) pixel pipeline~~ — done (v0.7): peak memory
   234 MB → 113 MB on an A4 300 dpi scan, output byte-identical.
9. ~~ICC/EXIF passthrough~~ — done (v0.11): `Encoder::icc_profile`
   (standard `ICC_PROFILE` APP2 chunking for profiles over 64 KB) and
   `Encoder::exif` attach metadata when encoding fresh pixels; the
   `optimize_jpeg` path was already preserving these segments. Both
   stable API.
10. ~~Perceptual metric (SSIM-class) in the bench~~ — done:
    SSIMULACRA2 and butteraugli scored every decision from v0.6 on.
11. ~~Lossless re-optimization of existing JPEGs~~ — done (v0.10),
    `optimize_jpeg` / `optimize_jpeg_baseline`: entropy-layer rewrite,
    pixels bit-identical, verified against independent decoders on
    real-world baseline, progressive, restart-interval, grayscale and
    4:2:2 files.

## License

MIT OR Apache-2.0, at your option.
