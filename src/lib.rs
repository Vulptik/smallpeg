//! smallpeg — a from-scratch, dependency-free JPEG encoder in pure Rust.
//!
//! Color transform, DCT, quantization and Huffman coding are all hand-rolled
//! from the JPEG standard (ITU-T T.81). No dependencies, no `unsafe`, and
//! `wasm32-unknown-unknown` is a first-class target.
//!
//! # What the encoder does by default
//!
//! - **Progressive, chosen by measurement.** The scan script is optimized
//!   per image (band split points, point-transform depth, DC precision are
//!   each priced with real statistics passes); both progressive and baseline
//!   entropy streams are then rendered and the smaller one ships. Set
//!   [`Encoder::progressive(false)`](Encoder::progressive) to force baseline
//!   sequential output for targets that cannot take progressive files.
//! - **Per-image optimized Huffman tables**, always on — the encoder is
//!   two-pass by design: pass 1 collects symbol statistics, pass 2 emits the
//!   entropy-coded stream, which is also the structure trellis quantization
//!   needs.
//! - **Trellis quantization.** Every AC coefficient is re-decided by dynamic
//!   programming against the real run-length/EOB structure; the DC
//!   coefficients form per-component DPCM chains and get their own Viterbi
//!   pass.
//! - **AAN scaled DCT**, with descaling folded into the quantization
//!   divisors and verified bit-for-bit against the naive orthonormal
//!   transform in tests.
//! - **Quality-adaptive quantization tables**, with two presets —
//!   [`QuantPreset::Photo`] (the default) and [`QuantPreset::AnnexK`].
//! - **Clipped-pixel deringing.** Pixels at full white decode to 255
//!   whatever value past 255 they are encoded as, so in blocks that mix
//!   white with darker content they are re-chosen as a smooth continuation
//!   of the neighboring edge rising past the clip point. Quantization
//!   ripple then lands above 255, where the decoder's clamp removes it,
//!   instead of showing as haze around text and line art. Photographs are
//!   essentially unaffected. Off at qualities where a quantization step
//!   reaches 1, where it could push coefficients out of baseline range.
//!
//! # Decoding
//!
//! [`decode_jpeg`] decodes grayscale and color JPEGs (baseline and
//! progressive, any sampling) to 8-bit gray or RGB, following the color
//! space conventions mainstream decoders use and their default
//! interpolating chroma upsampling; [`decode_cmyk`] does the same for
//! four-component files without color conversion (see below). Both take
//! untrusted input and return `None` rather than panic.
//!
//! # Example
//!
//! ```
//! let (width, height) = (64u32, 48u32);
//! let rgb = vec![128u8; (width * height * 3) as usize];
//!
//! let jpeg = smallpeg::Encoder::new(85)
//!     .sampling(smallpeg::Sampling::S420)
//!     .encode_rgb(&rgb, width, height);
//! assert_eq!(&jpeg[..2], &[0xFF, 0xD8]); // SOI
//!
//! // Downsample first when the target is "small" (usually it is):
//! let half = smallpeg::resize_rgb(&rgb, width, height, width / 2, height / 2);
//! let small = smallpeg::Encoder::new(80).encode_rgb(&half, width / 2, height / 2);
//! assert!(small.len() < jpeg.len());
//!
//! // Shrink an existing JPEG without touching its pixels:
//! if let Some(opt) = smallpeg::optimize_jpeg(&jpeg) {
//!     assert!(opt.len() <= jpeg.len());
//! }
//! ```
//!
//! # CMYK
//!
//! Four-component JPEGs (CMYK and YCCK, common in print-oriented PDFs) can
//! be decoded, resized and re-encoded without ever being converted to RGB:
//! [`decode_cmyk`] returns the stored channel values and the file's Adobe
//! APP14 coding, [`resize_cmyk`] filters each channel linearly, and
//! [`Encoder::encode_cmyk`] writes them back with an APP14 that matches
//! the coding it performed. Whether the values are inverted ("Adobe" CMYK)
//! is never interpreted, so it survives the round trip as long as the
//! decoded [`CmykImage::adobe`] is passed back.
//!
//! ```
//! # let src = smallpeg::Encoder::new(90).encode_cmyk(
//! #     &vec![128u8; 32 * 32 * 4], 32, 32, Some(smallpeg::AdobeTransform::Ycck));
//! let img = smallpeg::decode_cmyk(&src).expect("a four-component JPEG");
//! let (w, h) = (img.width / 2, img.height / 2);
//! let small = smallpeg::resize_cmyk(&img.pixels, img.width, img.height, w, h);
//! let jpeg = smallpeg::Encoder::new(85).encode_cmyk(&small, w, h, img.adobe);
//! # assert_eq!(smallpeg::decode_cmyk(&jpeg).unwrap().adobe, img.adobe);
//! ```
//!
//! # Color profiles
//!
//! The encoder takes raw RGB bytes and never sees the source file, so it
//! cannot know what color space those bytes are in. Output carries no ICC
//! profile unless you attach one with [`Encoder::icc_profile`], and
//! decoders then read it as sRGB. Pixels in another space (Display P3 is
//! the usual case: iPhone photos, macOS screenshots and renders) come out
//! with visibly shifted colors unless the source's profile is forwarded or
//! the pixels are converted to sRGB first. Most image decoding crates hand
//! back raw pixels and leave the profile aside, so it is easy to drop by
//! accident. [`optimize_jpeg`] is unaffected: it keeps the input's APPn
//! segments as they are.
//!
//! # Panics
//!
//! The encoding and resizing entry points take the pixel buffer and its
//! dimensions as separate arguments, and panic when they disagree — see the
//! `# Panics` section on each. [`optimize_jpeg`] and the decoders take
//! untrusted bytes instead and never panic: they return `None` for anything
//! they cannot handle safely. Memory follows the declared frame size,
//! not the file size, so for untrusted input check [`jpeg_info`] or pass
//! [`Limits`] with a pixel budget to the `_with` variants.

// The DSP-style loops in this file index several arrays with one shared
// position; the iterator forms clippy suggests obscure the math.
#![allow(clippy::needless_range_loop)]

mod cmyk;
mod decode;
mod dering;
mod meta;
mod optimize;
mod resize;
pub use cmyk::{AdobeTransform, CmykImage, decode_cmyk, decode_cmyk_with};
pub use decode::{DecodedImage, decode_jpeg, decode_jpeg_with};
pub use meta::{apply_orientation, strip_metadata};
pub use optimize::{
    JpegInfo, Limits, jpeg_info, optimize_jpeg, optimize_jpeg_baseline,
    optimize_jpeg_baseline_with, optimize_jpeg_with,
};
pub use resize::{
    CropRect, crop, fit_within, resize_cmyk, resize_gray, resize_gray_linear, resize_rgb,
    resize_rgb_linear,
};

#[cfg(all(feature = "parallel", target_arch = "wasm32"))]
compile_error!("feature \"parallel\" is not supported on wasm32 targets (no std threads)");

/// Chroma subsampling mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sampling {
    /// Full chroma resolution (4:4:4).
    S444,
    /// 2x2 chroma subsampling (4:2:0).
    S420,
}

/// Base quantization table preset (scaled by the `quality` setting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantPreset {
    /// The ITU T.81 Annex K example tables. The better pick for
    /// documents, scans, and noisy content at high quality.
    AnnexK,
    /// One fine-grained table shared by luma and chroma (widely used
    /// numeric data; provenance in THIRD_PARTY.md). Noticeably better on
    /// photographs — smooth gradients and low quality especially.
    Photo,
}

/// JPEG encoder, configured by chaining the builder methods below.
///
/// Built with [`Encoder::new`] from a quality value; every other setting has
/// a measured default, so `Encoder::new(q).encode_rgb(..)` is the intended
/// way to use this crate.
pub struct Encoder {
    quality: u8,
    sampling: Sampling,
    quant: QuantPreset,
    /// None = default lambda with the per-image gradient gate enabled;
    /// Some = fixed lambda, gate disabled (predictable for tuning).
    lambda: Option<f32>,
    deadzone: (f32, f32),
    /// AQ-lite (ringing control): (flat pixel-variance threshold, strong
    /// threshold, lambda gain < 1 applied to at-risk blocks, lambda boost
    /// >= 1 applied everywhere else). gain >= 1 disables.
    aq_lite: (f32, f32, f32, f32),
    /// Experimental table-shape tweaks: (chroma step scale, luma HF blend
    /// toward flat). (1.0, 0.0) = untouched presets.
    table_tweak: (f32, f32),
    /// None = pick automatically from the quant preset.
    chroma_lambda_scale: Option<f32>,
    /// Clipped-pixel deringing on luma: (overshoot cap in sample units,
    /// solver sweep limit, also treat black). None = off.
    dering: Option<(f32, u32, bool)>,
    /// CMYK deringing component mask override (None = per-coding default).
    cmyk_dering: Option<u8>,
    progressive: bool,
    icc: Option<Vec<u8>>,
    exif: Option<Vec<u8>>,
}

/// Default rate-distortion trade-off for trellis quantization, in units of
/// (quantization steps)^2 per bit. Tuned on the bench corpus: smaller keeps
/// more coefficients (higher PSNR), larger drops more (smaller files).
const DEFAULT_LAMBDA: f32 = 0.06;

/// AQ-lite (ringing control), kept off: reduced lambda at strong-edge /
/// flat-neighbor sites, optional boost elsewhere. Measured against the
/// SSIM2 + butteraugli-3-norm gate and rejected — the one-sided form is
/// bounded by nearest rounding (baseline JPEG has one table), and the
/// two-sided form pays more in global SSIM2 than it wins in worst-case
/// butteraugli, which as a max statistic does not respond consistently
/// to a binary block classifier. Left as a documented negative result.
const AQ_FLAT_VAR: f32 = 1.0;
const AQ_STRONG_VAR: f32 = 1000.0;
const AQ_GAIN: f32 = 1.0; // >= 1.0 = disabled
const AQ_BOOST: f32 = 1.0;

/// Adaptive deadzone (visual masking): busy blocks hide quantization
/// noise, so their trellis lambda is scaled up by (activity)^alpha, capped.
/// Flat blocks stay at the base lambda — nearest rounding is already the
/// most precise thing the syntax allows there, so masking is one-sided.
const DEADZONE_ALPHA: f32 = 0.0; // masking-direction boost lost to SSIM2; keep off
const DEADZONE_CAP: f32 = 2.0;

/// Quality-dependent luma high-frequency flattening (Photo preset
/// only): at high quality the preset's steep HF steps are too coarse
/// relative to its fine low band, and geometrically blending the table
/// toward flat wins on textured and document content. Gradient-dominant
/// images are exempt — measured per image from the quant-independent DCT
/// energy split. Ramp: 0 at q<=60 up to HF_BLEND_MAX at q>=90.
const HF_BLEND_MAX: f32 = 0.25;

/// Gradient-vs-texture classifier for the HF-blend exemption: gradients
/// concentrate pre-quant DCT energy in the first zigzag positions AND
/// quantize to very few nonzero ACs; texture fails at least one of the two.
/// Whether `is_gradient` will be consulted for this encoder's settings:
/// the quality-adaptive HF blend is active (Photo preset, untweaked tables,
/// quality above 60, i.e. a nonzero blend). Only then does the judged
/// component need its energy split accumulated.
fn tracks_gradient(e: &Encoder) -> bool {
    e.table_tweak == (1.0, 0.0) && e.quant == QuantPreset::Photo && e.quality > 60
}

fn is_gradient(c: &Component) -> bool {
    let ratio = c.e_low / c.e_all.max(f64::MIN_POSITIVE);
    let nz: usize = c
        .blocks
        .iter()
        .map(|b| b[1..].iter().filter(|&&v| v != 0).count())
        .sum();
    ratio >= 0.70 && nz as f64 / c.blocks.len() as f64 <= 6.0
}

/// Geometric blend of every step toward 16 — i.e. dynamic-range
/// compression of the table, not a pure HF flatten: steps above 16 get
/// finer, steps below 16 get coarser (reviewer's observation; measured a
/// net win with the LF sacrifice included).
fn compress_luma_table(qt: &[u16; 64], beta: f32) -> [u16; 64] {
    let mut out = *qt;
    for v in out.iter_mut() {
        let blended = ((*v as f32).ln() * (1.0 - beta) + (16f32).ln() * beta).exp();
        *v = (blended.round() as i32).clamp(1, 255) as u16;
    }
    out
}

/// Chroma trellis lambda scale per preset. Annex K's chroma steps are ~6x
/// coarser than luma, so step-unit distortion undervalues the real pixel
/// error of dropping a chroma coefficient — full-strength chroma trellis
/// cost ~2 dB for ~2% there, hence the tiny scale. The Photo preset
/// shares one fine-grained table across components, so its chroma trellis
/// is safe at half strength — and at full strength once the distortion is
/// frequency-weighted (`TRELLIS_FREQ_GAMMA_PHOTO`), which is how the RGB
/// path runs it.
const CHROMA_SCALE_ANNEXK: f32 = 0.05;
const CHROMA_SCALE_PHOTO: f32 = 0.5;
const CHROMA_SCALE_PHOTO_WEIGHTED: f32 = 1.0;

/// Frequency weighting of the AC trellis distortion for the Photo preset
/// on RGB input: coefficient k's squared step-unit error is multiplied by
/// (q_k / geometric mean of the AC steps)^gamma. Plain step units (gamma
/// 0) treat a one-step error the same everywhere; true pixel error (gamma
/// 2) wrecks gradients (see `trellis_ac`). A mild 0.5 makes coarse-step
/// high frequencies a little dearer to drop and fine-step low frequencies
/// a little cheaper, and with it the chroma trellis is best at the luma
/// lambda. Chosen on CLIC 2020 professional at 4:2:0 (0.35-1.0 plateau or
/// worse); on images held out from that choice (CLIC mobile, Kodak,
/// rendered documents) output is 0-2% smaller at 4:2:0 and 2-3.5% at
/// 4:4:4 at equal SSIMULACRA2 / butteraugli, at the same encode time.
/// Annex K, grayscale and CMYK keep plain step units (not measured).
const TRELLIS_FREQ_GAMMA_PHOTO: f32 = 0.5;

/// Chroma quant-step multiplier for the Photo preset at 4:4:4. Photo shares
/// one fine table between luma and chroma, which is balanced at 4:2:0; at
/// 4:4:4 chroma has four times the samples and that table overspends on
/// it. Doubling the chroma steps was 5-10% smaller at equal SSIMULACRA2
/// and 9-14% at equal butteraugli on photos held out from the choice of
/// factor (CLIC 2020 mobile, Kodak), and neutral to slightly better on
/// rendered documents. 2.5 and 3 won further at high quality but lost
/// around SSIMULACRA2 70, documents included. Measured up to quality 95;
/// above that it fades to 1 so quality 100 keeps all-1 tables.
fn photo_444_chroma_factor(quality: u8) -> f32 {
    (1.0 + (100.0 - quality as f32) / 5.0).clamp(1.0, 2.0)
}

/// Clipped-pixel deringing (see `dering`): luma pixels at full white may
/// be pushed up to this far past 255 along the neighboring edge's slope,
/// with at most this many solver sweeps. Measured on rendered documents
/// (Latin and Japanese text, tables, charts): 17-19% smaller at equal
/// SSIMULACRA2 and 10-13% at equal butteraugli, with 48 and 64 tied and
/// 32 slightly behind; photographs are neutral (within 0.2%). Black is
/// left alone: dark ink carried almost none of the gain.
const DERING_CAP: f32 = 48.0;
/// Hard limit on the overshoot, whatever `Encoder::dering` asks for: keeps
/// the coefficient-range argument in `make_dering` valid.
const DERING_CAP_MAX: f32 = 64.0;
/// Largest coefficient magnitude baseline JPEG can code (see `push_block`).
const COEF_MAX: f32 = 1023.0;
const DERING_SWEEPS: u32 = 10;

impl Encoder {
    /// New encoder at the given quality, on the usual 1-100 JPEG scale
    /// (values outside it are clamped, not rejected).
    ///
    /// Defaults: 4:2:0 subsampling, [`QuantPreset::Photo`] tables, trellis
    /// quantization on, clipped-pixel deringing on, progressive output.
    pub fn new(quality: u8) -> Self {
        Self {
            quality: quality.clamp(1, 100),
            sampling: Sampling::S420,
            quant: QuantPreset::Photo,
            lambda: None,
            deadzone: (DEADZONE_ALPHA, DEADZONE_CAP),
            aq_lite: (AQ_FLAT_VAR, AQ_STRONG_VAR, AQ_GAIN, AQ_BOOST),
            table_tweak: (1.0, 0.0),
            chroma_lambda_scale: None,
            dering: Some((DERING_CAP, DERING_SWEEPS, false)),
            cmyk_dering: None,
            progressive: true,
            icc: None,
            exif: None,
        }
    }

    /// Chroma subsampling mode. Defaults to [`Sampling::S420`].
    pub fn sampling(mut self, sampling: Sampling) -> Self {
        self.sampling = sampling;
        self
    }

    /// Choose the base quantization tables.
    pub fn quant_preset(mut self, preset: QuantPreset) -> Self {
        self.quant = preset;
        self
    }

    fn base_tables(&self) -> (&'static [u16; 64], &'static [u16; 64]) {
        match self.quant {
            QuantPreset::AnnexK => (&BASE_QT_LUMA, &BASE_QT_CHROMA),
            QuantPreset::Photo => (&BASE_QT_PHOTO, &BASE_QT_PHOTO),
        }
    }

    /// Progressive output is the default: the scan script is optimized per
    /// image and the result still has to beat the baseline render to ship.
    /// Pass `false` to force baseline sequential output (single scan,
    /// maximum decoder compatibility — e.g. JPEGs embedded in PDFs, where
    /// progressive support across viewers is not a safe bet).
    ///
    /// This method and the `false` (baseline) mode are stable API:
    /// downstream users pin their compatibility guarantees on it.
    pub fn progressive(mut self, on: bool) -> Self {
        self.progressive = on;
        self
    }

    /// Embed an ICC color profile (the raw profile bytes) in the output,
    /// as the standard `ICC_PROFILE` APP2 marker sequence — chunked when
    /// the profile exceeds one segment's capacity (65,519 bytes). Profiles
    /// larger than the format's hard limit of 255 chunks (~16.7 MB) are
    /// not emitted. Stable API.
    ///
    /// Without a profile, decoders treat the output as sRGB. When the
    /// pixels come from a non-sRGB source (Display P3 and the like),
    /// forward that source's profile here, or the colors shift. See
    /// [Color profiles](crate#color-profiles).
    ///
    /// ```
    /// # let (rgb, width, height) = (vec![0u8; 16 * 16 * 3], 16u32, 16u32);
    /// // e.g. the profile your image decoder read from the source file
    /// let source_icc: Option<Vec<u8>> = None;
    ///
    /// let mut enc = smallpeg::Encoder::new(85);
    /// if let Some(icc) = &source_icc {
    ///     enc = enc.icc_profile(icc);
    /// }
    /// let jpeg = enc.encode_rgb(&rgb, width, height);
    /// # assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
    /// ```
    pub fn icc_profile(mut self, profile: &[u8]) -> Self {
        self.icc = Some(profile.to_vec());
        self
    }

    /// Embed EXIF metadata (the TIFF stream, without the `Exif\0\0`
    /// header) as an APP1 marker. EXIF is defined as a single segment, so
    /// payloads over 65,527 bytes are not emitted. Stable API.
    pub fn exif(mut self, tiff: &[u8]) -> Self {
        self.exif = Some(tiff.to_vec());
        self
    }

    /// Render the APP1 (EXIF) and APP2 (ICC) segments placed right after
    /// the JFIF APP0 header, in that order — the layout common tools
    /// produce when a file carries both.
    fn app_markers(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if let Some(tiff) = self.exif.as_deref()
            && tiff.len() <= 65533 - 6
        {
            out.extend_from_slice(&[0xFF, 0xE1]);
            out.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
            out.extend_from_slice(b"Exif\0\0");
            out.extend_from_slice(tiff);
        }
        if let Some(icc) = self.icc.as_deref() {
            const CHUNK: usize = 65533 - 14; // len field + "ICC_PROFILE\0" + seq/count
            let chunks = icc.chunks(CHUNK.max(1));
            let n = chunks.len();
            if (1..=255).contains(&n) && !icc.is_empty() {
                for (i, c) in chunks.enumerate() {
                    out.extend_from_slice(&[0xFF, 0xE2]);
                    out.extend_from_slice(&((2 + 12 + 2 + c.len()) as u16).to_be_bytes());
                    out.extend_from_slice(b"ICC_PROFILE\0");
                    out.push(i as u8 + 1);
                    out.push(n as u8);
                    out.extend_from_slice(c);
                }
            }
        }
        out
    }

    /// Override the trellis rate-distortion lambda. Exposed for tuning;
    /// the default is right for almost everyone. An explicit lambda also
    /// disables the per-image gradient escalation, so measurements see
    /// exactly the value they set.
    #[doc(hidden)]
    pub fn trellis_lambda(mut self, lambda: f32) -> Self {
        self.lambda = Some(lambda.max(0.0));
        self
    }

    /// Override the chroma lambda scale. Exposed for tuning.
    #[doc(hidden)]
    pub fn trellis_chroma_scale(mut self, scale: f32) -> Self {
        self.chroma_lambda_scale = Some(scale.max(0.0));
        self
    }

    /// Override the AQ-lite ringing-control parameters. Exposed for tuning.
    #[doc(hidden)]
    pub fn aq_lite(mut self, flat_var: f32, strong_var: f32, gain: f32, boost: f32) -> Self {
        self.aq_lite = (
            flat_var.max(0.0),
            strong_var.max(0.0),
            gain.max(0.0),
            boost.max(1.0),
        );
        self
    }

    /// Override the adaptive-deadzone masking strength. Exposed for tuning.
    #[doc(hidden)]
    pub fn deadzone(mut self, alpha: f32, cap: f32) -> Self {
        self.deadzone = (alpha.max(0.0), cap.max(1.0));
        self
    }

    /// Experimental quant-table shape tweaks. Exposed for tuning.
    #[doc(hidden)]
    pub fn table_tweak(mut self, chroma_scale: f32, luma_hf_blend: f32) -> Self {
        self.table_tweak = (chroma_scale.max(0.05), luma_hf_blend.clamp(0.0, 1.0));
        self
    }

    /// Turn clipped-pixel deringing on (the default) or off.
    ///
    /// It pays off on content with exact-white areas next to hard edges:
    /// rendered documents, screenshots, line art (17–19% smaller at equal
    /// SSIMULACRA2 on rendered documents). Images that are themselves
    /// decoded JPEGs, such as those re-compressed out of PDFs, measured
    /// neutral while encoding about 25% slower, so turning it off there
    /// saves time at no cost. Stable API.
    pub fn deringing(mut self, on: bool) -> Self {
        self.dering = on.then_some((DERING_CAP, DERING_SWEEPS, false));
        self
    }

    /// Clipped-pixel deringing parameters. Exposed for tuning; pass
    /// `sweeps == 0` to turn it off.
    #[doc(hidden)]
    pub fn dering(mut self, cap: f32, sweeps: u32, low: bool) -> Self {
        self.dering = (sweeps > 0).then_some((cap, sweeps, low));
        self
    }

    /// Which components of `encode_cmyk` output are deringed (bit i =
    /// component i). Exposed for tuning.
    #[doc(hidden)]
    pub fn cmyk_dering(mut self, mask: u8) -> Self {
        self.cmyk_dering = Some(mask);
        self
    }

    /// Deringing for a component quantized with `qt`, if enabled. Off when
    /// any step is 1: samples pushed past the clip points widen a block's
    /// range beyond 255, and at step 1 its coefficients can then exceed
    /// what baseline JPEG can code (AC size 10, |c| <= 1023). With every
    /// step at 2 or more, even the widest overshot block (range 255 plus
    /// twice the cap) stays well inside. The ringing it removes is
    /// negligible at those qualities anyway.
    fn make_dering(&self, qt: &[u16; 64]) -> Option<dering::Dering> {
        if qt.iter().any(|&q| q < 2) {
            return None;
        }
        self.dering
            .map(|(cap, sweeps, low)| dering::Dering::new(cap.min(DERING_CAP_MAX), sweeps, low))
    }

    fn chroma_scale(&self) -> f32 {
        self.chroma_lambda_scale.unwrap_or(match self.quant {
            QuantPreset::AnnexK => CHROMA_SCALE_ANNEXK,
            QuantPreset::Photo => CHROMA_SCALE_PHOTO,
        })
    }

    /// (chroma lambda scale, distortion frequency-weight gamma) for the RGB
    /// path, where the Photo preset pairs weighting with a full-strength
    /// chroma trellis. An explicit `chroma_lambda_scale` still wins.
    fn rgb_trellis_tuning(&self) -> (f32, f32) {
        match self.quant {
            QuantPreset::AnnexK => (self.chroma_scale(), 0.0),
            QuantPreset::Photo => (
                self.chroma_lambda_scale
                    .unwrap_or(CHROMA_SCALE_PHOTO_WEIGHTED),
                TRELLIS_FREQ_GAMMA_PHOTO,
            ),
        }
    }

    /// Encode interleaved 8-bit RGB pixels.
    ///
    /// # Panics
    ///
    /// Panics if `rgb.len()` is not exactly `width * height * 3`, or if
    /// either dimension is 0 or above 65535 (the JPEG frame-header limit).
    pub fn encode_rgb(&self, rgb: &[u8], width: u32, height: u32) -> Vec<u8> {
        assert_eq!(rgb.len(), width as usize * height as usize * 3);
        assert!(width > 0 && height > 0 && width <= 65535 && height <= 65535);
        let (w, h) = (width as usize, height as usize);
        let (base_l, base_c) = self.base_tables();
        let mut qt_luma = scaled_quant_table(base_l, self.quality);
        let mut qt_chroma = scaled_quant_table(base_c, self.quality);
        let (tw_c, tw_b) = self.table_tweak;
        // The tweak overrides the automatic 4:4:4 chroma scale, so tuning
        // runs see exactly the factor they set.
        let chroma_factor = if tw_c != 1.0 {
            tw_c
        } else if self.sampling == Sampling::S444 && self.quant == QuantPreset::Photo {
            photo_444_chroma_factor(self.quality)
        } else {
            1.0
        };
        if chroma_factor != 1.0 {
            for v in qt_chroma.iter_mut() {
                *v = ((*v as f32 * chroma_factor).round() as i32).clamp(1, 255) as u16;
            }
        }
        if tw_b > 0.0 {
            for v in qt_luma.iter_mut() {
                let blended = ((*v as f32).ln() * (1.0 - tw_b) + (16f32).ln() * tw_b).exp();
                *v = (blended.round() as i32).clamp(1, 255) as u16;
            }
        }
        // Luma block grid must cover whole MCUs (2x2 blocks in 4:2:0).
        let (h2, v2) = match self.sampling {
            Sampling::S444 => (1usize, 1usize),
            Sampling::S420 => (2, 2),
        };
        let mcu_w = ceil_div(w, 8 * h2);
        let mcu_h = ceil_div(h, 8 * v2);
        let lbw = mcu_w * h2;
        let (cw, chh) = (ceil_div(w, h2), ceil_div(h, v2)); // chroma sample dims

        let auto_hf = self.table_tweak == (1.0, 0.0) && self.quant == QuantPreset::Photo;
        let beta_q = HF_BLEND_MAX * ((self.quality as f32 - 60.0) / 30.0).clamp(0.0, 1.0);

        let build = |qt_l: &[u16; 64], qt_c: &[u16; 64]| -> Vec<Component> {
            let recip_l = dequant_scale(qt_l);
            let recip_c = dequant_scale(qt_c);
            let der = self.make_dering(qt_l);

            let mut comps = vec![
                Component::with_grid(
                    lbw,
                    ceil_div(w, 8),
                    ceil_div(h, 8),
                    h2 as u8,
                    v2 as u8,
                    0,
                    lbw * mcu_h * v2,
                ),
                Component::with_grid(
                    mcu_w,
                    ceil_div(cw, 8),
                    ceil_div(chh, 8),
                    1,
                    1,
                    1,
                    mcu_w * mcu_h,
                ),
                Component::with_grid(
                    mcu_w,
                    ceil_div(cw, 8),
                    ceil_div(chh, 8),
                    1,
                    1,
                    1,
                    mcu_w * mcu_h,
                ),
            ];
            comps[0].energy = tracks_gradient(self);

            // Color-convert and DCT one MCU row at a time: the strips below are
            // the only pixel-domain buffers, so peak memory stays proportional
            // to the coefficients, not to three full f32 planes. Strips are
            // padded to whole MCUs (edge replication baked in) so block
            // extraction is branch-free.
            let ypw = lbw * 8;
            let cpw = mcu_w * 8;
            let mut ystrip = vec![0f32; ypw * 8 * v2];
            let mut cbstrip = vec![0f32; cpw * 8];
            let mut crstrip = vec![0f32; cpw * 8];
            let mut px = [0f32; 64];
            for my in 0..mcu_h {
                for r in 0..8 * v2 {
                    let sy = (my * 8 * v2 + r).min(h - 1);
                    let src = &rgb[sy * w * 3..][..w * 3];
                    let dst = &mut ystrip[r * ypw..][..ypw];
                    for x in 0..w {
                        let (rr, gg, bb) = (
                            src[x * 3] as f32,
                            src[x * 3 + 1] as f32,
                            src[x * 3 + 2] as f32,
                        );
                        dst[x] = 0.299 * rr + 0.587 * gg + 0.114 * bb;
                    }
                    let last = dst[w - 1];
                    dst[w..].fill(last);
                }
                let cbcr = |p: &[u8]| -> (f32, f32) {
                    let (rr, gg, bb) = (p[0] as f32, p[1] as f32, p[2] as f32);
                    (
                        -0.168736 * rr - 0.331264 * gg + 0.5 * bb + 128.0,
                        0.5 * rr - 0.418688 * gg - 0.081312 * bb + 128.0,
                    )
                };
                if h2 == 2 {
                    // 4:2:0 — per-pixel conversion, then the same box average
                    // (identical arithmetic order to the old plane pipeline).
                    for r in 0..8 {
                        // Clamp in chroma sample space so padded rows replicate
                        // the last real chroma row (not a recomputed variant).
                        let cy = (my * 8 + r).min(chh - 1);
                        let y0 = (cy * 2).min(h - 1);
                        let y1 = (cy * 2 + 1).min(h - 1);
                        for cx in 0..cw {
                            let x0 = (cx * 2).min(w - 1);
                            let x1 = (cx * 2 + 1).min(w - 1);
                            let (b00, r00) = cbcr(&rgb[(y0 * w + x0) * 3..][..3]);
                            let (b01, r01) = cbcr(&rgb[(y0 * w + x1) * 3..][..3]);
                            let (b10, r10) = cbcr(&rgb[(y1 * w + x0) * 3..][..3]);
                            let (b11, r11) = cbcr(&rgb[(y1 * w + x1) * 3..][..3]);
                            cbstrip[r * cpw + cx] = (b00 + b01 + b10 + b11) * 0.25;
                            crstrip[r * cpw + cx] = (r00 + r01 + r10 + r11) * 0.25;
                        }
                        let (lb, lr) = (cbstrip[r * cpw + cw - 1], crstrip[r * cpw + cw - 1]);
                        cbstrip[r * cpw + cw..(r + 1) * cpw].fill(lb);
                        crstrip[r * cpw + cw..(r + 1) * cpw].fill(lr);
                    }
                } else {
                    for r in 0..8 {
                        let sy = (my * 8 + r).min(h - 1);
                        for x in 0..w {
                            let (b, rr) = cbcr(&rgb[(sy * w + x) * 3..][..3]);
                            cbstrip[r * cpw + x] = b;
                            crstrip[r * cpw + x] = rr;
                        }
                        let (lb, lr) = (cbstrip[r * cpw + w - 1], crstrip[r * cpw + w - 1]);
                        cbstrip[r * cpw + w..(r + 1) * cpw].fill(lb);
                        crstrip[r * cpw + w..(r + 1) * cpw].fill(lr);
                    }
                }
                for br in 0..v2 {
                    for bx in 0..lbw {
                        for yy in 0..8 {
                            let row = &ystrip[(br * 8 + yy) * ypw + bx * 8..][..8];
                            for (d, &sv) in px[yy * 8..yy * 8 + 8].iter_mut().zip(row) {
                                *d = sv - 128.0;
                            }
                        }
                        if let Some(d) = &der {
                            d.apply(&mut px);
                        }
                        comps[0].push_block(&mut px, &recip_l);
                    }
                }
                for (ci, strip) in [(1usize, &cbstrip), (2, &crstrip)] {
                    for bx in 0..mcu_w {
                        for yy in 0..8 {
                            let row = &strip[yy * cpw + bx * 8..][..8];
                            for (d, &sv) in px[yy * 8..yy * 8 + 8].iter_mut().zip(row) {
                                *d = sv - 128.0;
                            }
                        }
                        comps[ci].push_block(&mut px, &recip_c);
                    }
                }
            }

            comps
        };

        let mut qt_used = if auto_hf && beta_q > 0.0 {
            compress_luma_table(&qt_luma, beta_q)
        } else {
            qt_luma
        };
        let mut comps = build(&qt_used, &qt_chroma);
        if auto_hf && beta_q > 0.0 && is_gradient(&comps[0]) {
            // Gradient-dominant image: the flattened HF only wastes
            // bits here. Rebuild with the untouched preset shape.
            qt_used = qt_luma;
            comps = build(&qt_used, &qt_chroma);
        }
        let order = interleave_order(&comps, mcu_w, mcu_h);
        let (chroma_scale, freq_gamma) = self.rgb_trellis_tuning();
        encode_document(
            comps,
            &order,
            &[qt_used, qt_chroma],
            width as u16,
            height as u16,
            &self.app_markers(),
            self.lambda.unwrap_or(DEFAULT_LAMBDA),
            chroma_scale,
            freq_gamma,
            self.progressive,
            self.lambda.is_none(),
            self.deadzone,
            self.aq_lite,
            true,
        )
    }

    /// Encode 8-bit grayscale pixels.
    ///
    /// # Panics
    ///
    /// Panics if `gray.len()` is not exactly `width * height`, or if either
    /// dimension is 0 or above 65535 (the JPEG frame-header limit).
    pub fn encode_gray(&self, gray: &[u8], width: u32, height: u32) -> Vec<u8> {
        assert_eq!(gray.len(), width as usize * height as usize);
        assert!(width > 0 && height > 0 && width <= 65535 && height <= 65535);
        let (w, h) = (width as usize, height as usize);
        let qt = scaled_quant_table(self.base_tables().0, self.quality);
        let bw = ceil_div(w, 8);
        let bh = ceil_div(h, 8);
        let build = |qt_l: &[u16; 64]| -> Component {
            let recip = dequant_scale(qt_l);
            let der = self.make_dering(qt_l);
            let mut c = Component::with_grid(bw, bw, bh, 1, 1, 0, bw * bh);
            c.energy = tracks_gradient(self);
            let mut px = [0f32; 64];
            for by in 0..bh {
                for bx in 0..bw {
                    if bx * 8 + 8 <= w && by * 8 + 8 <= h {
                        for yy in 0..8 {
                            let row = &gray[(by * 8 + yy) * w + bx * 8..][..8];
                            for (d, &sv) in px[yy * 8..yy * 8 + 8].iter_mut().zip(row) {
                                *d = sv as f32 - 128.0;
                            }
                        }
                    } else {
                        for yy in 0..8 {
                            for xx in 0..8 {
                                let sx = (bx * 8 + xx).min(w - 1);
                                let sy = (by * 8 + yy).min(h - 1);
                                px[yy * 8 + xx] = gray[sy * w + sx] as f32 - 128.0;
                            }
                        }
                    }
                    if let Some(d) = &der {
                        d.apply(&mut px);
                    }
                    c.push_block(&mut px, &recip);
                }
            }
            c
        };
        let auto_hf = self.table_tweak == (1.0, 0.0) && self.quant == QuantPreset::Photo;
        let beta_q = HF_BLEND_MAX * ((self.quality as f32 - 60.0) / 30.0).clamp(0.0, 1.0);
        let mut qt_used = if auto_hf && beta_q > 0.0 {
            compress_luma_table(&qt, beta_q)
        } else {
            qt
        };
        let mut c = build(&qt_used);
        if auto_hf && beta_q > 0.0 && is_gradient(&c) {
            qt_used = qt;
            c = build(&qt_used);
        }
        let order: Vec<(usize, usize)> = (0..bw * bh).map(|i| (0, i)).collect();
        encode_document(
            vec![c],
            &order,
            &[qt_used],
            width as u16,
            height as u16,
            &self.app_markers(),
            self.lambda.unwrap_or(DEFAULT_LAMBDA),
            self.chroma_scale(),
            0.0,
            self.progressive,
            self.lambda.is_none(),
            self.deadzone,
            self.aq_lite,
            true,
        )
    }
}

// ---------------------------------------------------------------- helpers

fn ceil_div(a: usize, b: usize) -> usize {
    a.div_ceil(b)
}

// ---------------------------------------------------------------- DCT

/// One 8-point pass of the AAN (Arai–Agui–Nakajima 1988) scaled DCT:
/// 5 multiplications, 29 additions. Output k is the true DCT coefficient
/// times 2√2·(√2·cos(kπ/16)) (and times 2√2 for k = 0); the descaling is
/// folded into the quantization divisors, so it never costs anything.
#[inline]
fn aan_pass(x: [f32; 8]) -> [f32; 8] {
    const C4: f32 = std::f32::consts::FRAC_1_SQRT_2; // cos(4π/16)
    const C6: f32 = 0.38268343; // cos(6π/16)
    const C2M6: f32 = 0.5411961; // cos(2π/16) - cos(6π/16)
    const C2P6: f32 = 1.3065629; // cos(2π/16) + cos(6π/16)

    let tmp0 = x[0] + x[7];
    let tmp7 = x[0] - x[7];
    let tmp1 = x[1] + x[6];
    let tmp6 = x[1] - x[6];
    let tmp2 = x[2] + x[5];
    let tmp5 = x[2] - x[5];
    let tmp3 = x[3] + x[4];
    let tmp4 = x[3] - x[4];

    // Even part.
    let tmp10 = tmp0 + tmp3;
    let tmp13 = tmp0 - tmp3;
    let tmp11 = tmp1 + tmp2;
    let tmp12 = tmp1 - tmp2;
    let z1 = (tmp12 + tmp13) * C4;

    // Odd part.
    let t10 = tmp4 + tmp5;
    let t11 = tmp5 + tmp6;
    let t12 = tmp6 + tmp7;
    let z5 = (t10 - t12) * C6;
    let z2 = C2M6 * t10 + z5;
    let z4 = C2P6 * t12 + z5;
    let z3 = t11 * C4;
    let z11 = tmp7 + z3;
    let z13 = tmp7 - z3;

    [
        tmp10 + tmp11,
        z11 + z4,
        tmp13 + z1,
        z13 - z2,
        tmp10 - tmp11,
        z13 + z2,
        tmp13 - z1,
        z11 - z4,
    ]
}

/// In-place 2D scaled DCT: AAN over rows, then over columns.
fn fdct8x8(block: &mut [f32; 64]) {
    for row in block.as_chunks_mut::<8>().0 {
        *row = aan_pass(*row);
    }
    for x in 0..8 {
        let mut col = [0f32; 8];
        for (y, c) in col.iter_mut().enumerate() {
            *c = block[y * 8 + x];
        }
        let out = aan_pass(col);
        for (y, &v) in out.iter().enumerate() {
            block[y * 8 + x] = v;
        }
    }
}

/// Combined dequantization + AAN descaling multipliers, natural order:
/// recip[i] = 1 / (qt[i] * 8 * s(row) * s(col)) with s(0) = 1 and
/// s(k) = √2·cos(kπ/16), so that `coeff * recip` equals DCT/qt exactly as
/// the naive orthonormal transform produced it.
fn dequant_scale(qt: &[u16; 64]) -> [f32; 64] {
    let mut s = [0f32; 8];
    for (k, v) in s.iter_mut().enumerate() {
        *v = if k == 0 {
            1.0
        } else {
            std::f32::consts::SQRT_2 * (k as f32 * std::f32::consts::PI / 16.0).cos()
        };
    }
    let mut out = [0f32; 64];
    for (i, o) in out.iter_mut().enumerate() {
        *o = 1.0 / (qt[i] as f32 * 8.0 * s[i / 8] * s[i % 8]);
    }
    out
}

// ---------------------------------------------------------------- quantization

#[rustfmt::skip]
const BASE_QT_LUMA: [u16; 64] = [
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
const BASE_QT_CHROMA: [u16; 64] = [
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
const BASE_QT_PHOTO: [u16; 64] = [
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
const ZIGZAG: [usize; 64] = [
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
fn scaled_quant_table(base: &[u16; 64], quality: u8) -> [u16; 64] {
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

// ---------------------------------------------------------------- components

struct Component {
    /// Quantized coefficients per block, already in zigzag order.
    blocks: Vec<[i16; 64]>,
    /// Pre-rounding coefficients (DCT value / quant step), zigzag order.
    /// Kept so trellis quantization can price alternative roundings.
    raw: Vec<[f32; 64]>,
    /// 0 = luma tables, 1 = chroma tables.
    table_class: usize,
    /// Width of the (MCU-padded) block grid `blocks` is stored in.
    bw: usize,
    /// Actual block count covering the component's samples. In 4:2:0 the
    /// luma grid is padded to whole MCUs; non-interleaved (progressive AC)
    /// scans must cover exactly these blocks and skip the padding.
    aw: usize,
    ah: usize,
    h: u8,
    v: u8,
    qt_id: u8,
    /// Pre-quantization DCT AC energy split (zigzag 1-2 vs all), i.e. a
    /// quant-table-independent gradient-vs-texture signal.
    e_low: f64,
    e_all: f64,
    /// Accumulate `e_low` / `e_all` in `push_block` (only the component
    /// `is_gradient` will judge needs them).
    energy: bool,
}

impl Component {
    fn with_grid(
        bw: usize,
        aw: usize,
        ah: usize,
        h: u8,
        v: u8,
        qt_id: u8,
        nblocks: usize,
    ) -> Component {
        Component {
            blocks: Vec::with_capacity(nblocks),
            raw: Vec::with_capacity(nblocks),
            table_class: if qt_id == 0 { 0 } else { 1 },
            bw,
            aw,
            ah,
            h,
            v,
            qt_id,
            e_low: 0.0,
            e_all: 0.0,
            energy: false,
        }
    }

    /// DCT + quantize one level-shifted 8x8 block and append it.
    fn push_block(&mut self, px: &mut [f32; 64], recip: &[f32; 64]) {
        fdct8x8(px);
        let mut zz = [0i16; 64];
        let mut t = [0f32; 64];
        for i in 0..64 {
            // Baseline 8-bit JPEG codes AC magnitudes up to 1023 (size 10)
            // and DC values in [-1024, 1023]. 8-bit samples stay inside
            // that by construction; this guards anything that could push
            // past it (samples beyond the clip points, rounding at step 1),
            // since strict decoders reject the whole file otherwise.
            t[i] = (px[ZIGZAG[i]] * recip[ZIGZAG[i]]).clamp(-COEF_MAX, COEF_MAX);
            zz[i] = t[i].round() as i16;
        }
        if self.energy {
            for i in 1..64 {
                let e = (px[ZIGZAG[i]] * px[ZIGZAG[i]]) as f64;
                self.e_all += e;
                if i <= 5 {
                    self.e_low += e;
                }
            }
        }
        self.blocks.push(zz);
        self.raw.push(t);
    }
}

/// Block emission order for an interleaved scan: per MCU, each component's
/// sampling region left-to-right, top-to-bottom.
fn interleave_order(comps: &[Component], mcu_w: usize, mcu_h: usize) -> Vec<(usize, usize)> {
    let mut order = Vec::new();
    for my in 0..mcu_h {
        for mx in 0..mcu_w {
            for (ci, c) in comps.iter().enumerate() {
                let bw = mcu_w * c.h as usize;
                for dy in 0..c.v as usize {
                    for dx in 0..c.h as usize {
                        let bx = mx * c.h as usize + dx;
                        let by = my * c.v as usize + dy;
                        order.push((ci, by * bw + bx));
                    }
                }
            }
        }
    }
    order
}

// ---------------------------------------------------------------- symbols

/// Bit category of a coefficient value (0..=11 for baseline).
#[inline]
fn category(v: i32) -> u8 {
    (32 - v.unsigned_abs().leading_zeros()) as u8
}

/// Amplitude bits: negatives are stored as value-1 in `size` low bits.
#[inline]
fn amplitude(v: i32, size: u8) -> u16 {
    if v < 0 {
        ((v - 1) & ((1i32 << size) - 1)) as u16
    } else {
        v as u16
    }
}

/// Walk every entropy symbol of the scan. `f(table_id, symbol, nbits, bits)`
/// where table_id = table_class * 2 + (0 for DC, 1 for AC).
fn for_each_symbol(
    comps: &[Component],
    order: &[(usize, usize)],
    mut f: impl FnMut(usize, u8, u8, u16),
) {
    let mut prev_dc = vec![0i32; comps.len()];
    for &(ci, bi) in order {
        let c = &comps[ci];
        let blk = &c.blocks[bi];
        let dc_tbl = c.table_class * 2;
        let ac_tbl = c.table_class * 2 + 1;

        let dc = blk[0] as i32;
        let diff = dc - prev_dc[ci];
        prev_dc[ci] = dc;
        let s = category(diff);
        f(dc_tbl, s, s, amplitude(diff, s));

        // Visit nonzero AC positions only: a bitmask built branch-free,
        // then walked with trailing_zeros (most blocks are sparse).
        let mut nz = 0u64;
        for k in 1..64 {
            nz |= ((blk[k] != 0) as u64) << k;
        }
        let mut next = 1u32; // first position not yet coded
        while nz != 0 {
            let k = nz.trailing_zeros();
            nz &= nz - 1;
            let mut run = (k - next) as u8;
            while run > 15 {
                f(ac_tbl, 0xF0, 0, 0); // ZRL
                run -= 16;
            }
            let v = blk[k as usize] as i32;
            let s = category(v);
            f(ac_tbl, (run << 4) | s, s, amplitude(v, s));
            next = k + 1;
        }
        if next < 64 {
            f(ac_tbl, 0x00, 0, 0); // EOB
        }
    }
}

/// DC symbols of a progressive first scan with successive-approximation
/// point transform `al` (arithmetic shift, per T.81). `f(table_class,
/// symbol, nbits, bits)`.
fn for_each_dc_symbol(
    comps: &[Component],
    order: &[(usize, usize)],
    al: u8,
    mut f: impl FnMut(usize, u8, u8, u16),
) {
    let mut prev_dc = vec![0i32; comps.len()];
    for &(ci, bi) in order {
        let c = &comps[ci];
        let dc = (c.blocks[bi][0] as i32) >> al;
        let diff = dc - prev_dc[ci];
        prev_dc[ci] = dc;
        let s = category(diff);
        f(c.table_class, s, s, amplitude(diff, s));
    }
}

/// AC symbols of one spectral band, first scan (Ah = 0) with point
/// transform `al`: coefficients enter as sign * (|v| >> al). Blocks whose
/// band is entirely zero accumulate into EOB runs: one EOBn symbol prices
/// a whole run of empty blocks, which is where progressive beats baseline
/// on smooth content.
#[allow(dead_code)] // reference implementation; the sparse walker must match it
fn for_each_ac_band_symbol(
    c: &Component,
    ss: usize,
    se: usize,
    al: u8,
    mut f: impl FnMut(u8, u8, u16),
) {
    let mut eobrun: u32 = 0;
    macro_rules! flush_eobrun {
        () => {
            if eobrun > 0 {
                let r = 31 - eobrun.leading_zeros();
                f((r << 4) as u8, r as u8, (eobrun & ((1 << r) - 1)) as u16);
                #[allow(unused_assignments)]
                {
                    eobrun = 0;
                }
            }
        };
    }
    for by in 0..c.ah {
        for bx in 0..c.aw {
            let blk = &c.blocks[by * c.bw + bx];
            let mut run = 0u32;
            let mut first = true;
            for k in ss..=se {
                let raw = blk[k] as i32;
                let mag = (raw.unsigned_abs() >> al) as i32;
                if mag == 0 {
                    run += 1;
                    continue;
                }
                let v = if raw < 0 { -mag } else { mag };
                if first {
                    flush_eobrun!();
                    first = false;
                }
                while run > 15 {
                    f(0xF0, 0, 0); // ZRL
                    run -= 16;
                }
                let s = category(v);
                f(((run as u8) << 4) | s, s, amplitude(v, s));
                run = 0;
            }
            if run > 0 {
                eobrun += 1;
                if eobrun == 32767 {
                    flush_eobrun!();
                }
            }
        }
    }
    flush_eobrun!();
}

/// AC refinement scan (Ah = al + 1, Al = al), T.81 G.1.2.3: coefficients
/// already significant at Ah get one correction bit; coefficients whose
/// bit `al` is their first nonzero bit are "newly significant" and coded
/// as (run, 1) symbols. Correction bits ride the entropy stream appended
/// to whichever symbol's span crosses their position — that ordering is
/// exactly what the decoder replays, so ZRLs must be emitted the moment a
/// zero-history run hits 16, not deferred.
///
/// `f(Some(sym), nbits, bits)` is a Huffman symbol with its immediate
/// payload; `f(None, 1, bit)` is an appended raw correction bit.
#[allow(dead_code)] // reference implementation; the sparse walker must match it
fn for_each_ac_refine_symbol(
    c: &Component,
    ss: usize,
    se: usize,
    al: u8,
    mut f: impl FnMut(Option<u8>, u8, u16),
) {
    let hi = al + 1;
    let mut eobrun: u32 = 0;
    let mut eob_bits: Vec<u16> = Vec::new();
    macro_rules! flush_eobrun {
        () => {
            if eobrun > 0 {
                let r = 31 - eobrun.leading_zeros();
                f(
                    Some((r << 4) as u8),
                    r as u8,
                    (eobrun & ((1 << r) - 1)) as u16,
                );
                for &b in &eob_bits {
                    f(None, 1, b);
                }
                eob_bits.clear();
                #[allow(unused_assignments)]
                {
                    eobrun = 0;
                }
            }
        };
    }
    for by in 0..c.ah {
        for bx in 0..c.aw {
            let blk = &c.blocks[by * c.bw + bx];
            // Last position that becomes significant in this scan; only
            // spans before it may emit ZRL (a trailing ZRL would desync
            // the decoder's EOB handling).
            let mut last_sig: Option<usize> = None;
            for k in ss..=se {
                let a = (blk[k] as i32).unsigned_abs();
                if (a >> hi) == 0 && (a >> al) != 0 {
                    last_sig = Some(k);
                }
            }
            let tail_from = match last_sig {
                None => ss,
                Some(ls) => {
                    flush_eobrun!();
                    let mut run = 0u32;
                    let mut corr: Vec<u16> = Vec::new();
                    for k in ss..=ls {
                        let raw = blk[k] as i32;
                        let a = raw.unsigned_abs();
                        if (a >> hi) != 0 {
                            corr.push(((a >> al) & 1) as u16);
                        } else if (a >> al) == 0 {
                            run += 1;
                            if run == 16 {
                                f(Some(0xF0), 0, 0);
                                for &b in &corr {
                                    f(None, 1, b);
                                }
                                corr.clear();
                                run = 0;
                            }
                        } else {
                            f(Some(((run as u8) << 4) | 1), 1, if raw > 0 { 1 } else { 0 });
                            for &b in &corr {
                                f(None, 1, b);
                            }
                            corr.clear();
                            run = 0;
                        }
                    }
                    if ls == se {
                        continue; // fully coded, no EOB contribution
                    }
                    ls + 1
                }
            };
            // Remaining positions join the EOB run; their correction bits
            // are buffered and follow the EOBn symbol.
            for k in tail_from..=se {
                let a = (blk[k] as i32).unsigned_abs();
                if (a >> hi) != 0 {
                    eob_bits.push(((a >> al) & 1) as u16);
                }
            }
            eobrun += 1;
            if eobrun == 32767 {
                flush_eobrun!();
            }
        }
    }
    flush_eobrun!();
}

/// CSR sparse view of a component's AC coefficients in scan (by,bx) order:
/// offsets[i]..offsets[i+1] indexes (zigzag position, value) pairs.
struct SparseAc {
    offsets: Vec<u32>,
    entries: Vec<(u8, i16)>,
}

fn sparse_ac(c: &Component) -> SparseAc {
    let n = c.aw * c.ah;
    let mut offsets = Vec::with_capacity(n + 1);
    let mut entries = Vec::new();
    offsets.push(0u32);
    for by in 0..c.ah {
        for bx in 0..c.aw {
            let blk = &c.blocks[by * c.bw + bx];
            for (k, &v) in blk.iter().enumerate().skip(1) {
                if v != 0 {
                    entries.push((k as u8, v));
                }
            }
            offsets.push(entries.len() as u32);
        }
    }
    SparseAc { offsets, entries }
}

/// Sparse equivalent of `for_each_ac_band_symbol`: same symbol stream,
/// but touches only nonzero coefficients.
fn for_each_ac_band_symbol_sparse(
    s: &SparseAc,
    ss: usize,
    se: usize,
    al: u8,
    mut f: impl FnMut(u8, u8, u16),
) {
    let mut eobrun: u32 = 0;
    macro_rules! flush_eobrun {
        () => {
            if eobrun > 0 {
                let r = 31 - eobrun.leading_zeros();
                f((r << 4) as u8, r as u8, (eobrun & ((1 << r) - 1)) as u16);
                #[allow(unused_assignments)]
                {
                    eobrun = 0;
                }
            }
        };
    }
    for w in s.offsets.windows(2) {
        let ents = &s.entries[w[0] as usize..w[1] as usize];
        let mut last = ss - 1; // position of last emitted nonzero
        let mut first = true;
        for &(kp, v) in ents {
            let k = kp as usize;
            if k < ss || k > se {
                continue;
            }
            let mag = ((v as i32).unsigned_abs() >> al) as i32;
            if mag == 0 {
                continue;
            }
            let vv = if v < 0 { -mag } else { mag };
            if first {
                flush_eobrun!();
                first = false;
            }
            let mut run = (k - last - 1) as u32;
            while run > 15 {
                f(0xF0, 0, 0); // ZRL
                run -= 16;
            }
            let sz = category(vv);
            f(((run as u8) << 4) | sz, sz, amplitude(vv, sz));
            last = k;
        }
        if last < se {
            eobrun += 1;
            if eobrun == 32767 {
                flush_eobrun!();
            }
        }
    }
    flush_eobrun!();
}

/// Sparse equivalent of `for_each_ac_refine_symbol`.
fn for_each_ac_refine_symbol_sparse(
    s: &SparseAc,
    ss: usize,
    se: usize,
    al: u8,
    mut f: impl FnMut(Option<u8>, u8, u16),
) {
    let hi = al + 1;
    let mut eobrun: u32 = 0;
    let mut eob_bits: Vec<u16> = Vec::new();
    macro_rules! flush_eobrun {
        () => {
            if eobrun > 0 {
                let r = 31 - eobrun.leading_zeros();
                f(
                    Some((r << 4) as u8),
                    r as u8,
                    (eobrun & ((1 << r) - 1)) as u16,
                );
                for &b in &eob_bits {
                    f(None, 1, b);
                }
                eob_bits.clear();
                #[allow(unused_assignments)]
                {
                    eobrun = 0;
                }
            }
        };
    }
    for w in s.offsets.windows(2) {
        let ents: Vec<(usize, i32)> = s.entries[w[0] as usize..w[1] as usize]
            .iter()
            .filter(|&&(k, _)| (k as usize) >= ss && (k as usize) <= se)
            .map(|&(k, v)| (k as usize, v as i32))
            .collect();
        let mut last_sig: Option<usize> = None;
        for &(k, v) in &ents {
            let a = v.unsigned_abs();
            if (a >> hi) == 0 && (a >> al) != 0 {
                last_sig = Some(k);
            }
        }
        let tail_from = match last_sig {
            None => ss,
            Some(ls) => {
                flush_eobrun!();
                let mut run = 0u32;
                let mut corr: Vec<u16> = Vec::new();
                let mut pos = ss; // next position to account for
                for &(k, v) in ents.iter().filter(|&&(k, _)| k <= ls) {
                    let a = v.unsigned_abs();
                    // true zeros in the gap [pos, k)
                    for _ in pos..k {
                        run += 1;
                        if run == 16 {
                            f(Some(0xF0), 0, 0);
                            for &b in &corr {
                                f(None, 1, b);
                            }
                            corr.clear();
                            run = 0;
                        }
                    }
                    pos = k + 1;
                    if (a >> hi) != 0 {
                        corr.push(((a >> al) & 1) as u16);
                    } else if (a >> al) == 0 {
                        run += 1;
                        if run == 16 {
                            f(Some(0xF0), 0, 0);
                            for &b in &corr {
                                f(None, 1, b);
                            }
                            corr.clear();
                            run = 0;
                        }
                    } else {
                        f(Some(((run as u8) << 4) | 1), 1, if v > 0 { 1 } else { 0 });
                        for &b in &corr {
                            f(None, 1, b);
                        }
                        corr.clear();
                        run = 0;
                    }
                }
                if ls == se {
                    continue;
                }
                ls + 1
            }
        };
        for &(_, v) in ents.iter().filter(|&&(k, _)| k >= tail_from) {
            let a = v.unsigned_abs();
            if (a >> hi) != 0 {
                eob_bits.push(((a >> al) & 1) as u16);
            }
        }
        eobrun += 1;
        if eobrun == 32767 {
            flush_eobrun!();
        }
    }
    flush_eobrun!();
}

/// Cost statistics of one band for al = 0, 1, 2 in a single sparse pass:
/// returns per-al (symbol freq table, amplitude-bit payload). Exactly the
/// same counts as three separate `for_each_ac_band_symbol_sparse` passes.
fn ac_band_stats_all_al(s: &SparseAc, ss: usize, se: usize) -> [([u32; 256], u64); 3] {
    struct St {
        freq: [u32; 256],
        payload: u64,
        eobrun: u32,
    }
    let mut st = [
        St {
            freq: [0; 256],
            payload: 0,
            eobrun: 0,
        },
        St {
            freq: [0; 256],
            payload: 0,
            eobrun: 0,
        },
        St {
            freq: [0; 256],
            payload: 0,
            eobrun: 0,
        },
    ];
    #[inline]
    fn flush(st: &mut St) {
        if st.eobrun > 0 {
            let r = 31 - st.eobrun.leading_zeros();
            st.freq[(r << 4) as usize] += 1;
            st.payload += r as u64;
            st.eobrun = 0;
        }
    }
    for w in s.offsets.windows(2) {
        let ents = &s.entries[w[0] as usize..w[1] as usize];
        let mut last = [ss - 1; 3];
        let mut first = [true; 3];
        for &(kp, v) in ents {
            let k = kp as usize;
            if k < ss || k > se {
                continue;
            }
            let a = (v as i32).unsigned_abs();
            for al in 0..3usize {
                let mag = (a >> al) as i32;
                if mag == 0 {
                    continue;
                }
                let stx = &mut st[al];
                if first[al] {
                    flush(stx);
                    first[al] = false;
                }
                let mut run = (k - last[al] - 1) as u32;
                while run > 15 {
                    stx.freq[0xF0] += 1;
                    run -= 16;
                }
                let sz = category(mag);
                stx.freq[((run as usize) << 4) | sz as usize] += 1;
                stx.payload += sz as u64;
                last[al] = k;
            }
        }
        for al in 0..3usize {
            if last[al] < se {
                st[al].eobrun += 1;
                if st[al].eobrun == 32767 {
                    flush(&mut st[al]);
                }
            }
        }
    }
    for stx in st.iter_mut() {
        flush(stx);
    }
    [
        (st[0].freq, st[0].payload),
        (st[1].freq, st[1].payload),
        (st[2].freq, st[2].payload),
    ]
}

// ---------------------------------------------------------------- trellis

/// Estimated code length in bits for every symbol, from pass-1 frequencies.
/// Entropy with add-half smoothing, so symbols the trellis wants to try but
/// pass 1 never produced still get a finite (pessimistic) price.
fn estimate_bits(freq: &[u32; 256]) -> [f32; 256] {
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
fn trellis_ac(
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

// ---------------------------------------------------------------- Huffman

struct HuffTable {
    /// bits[i] = number of codes of length i (1-indexed, up to 16).
    bits: [u8; 17],
    values: Vec<u8>,
    /// codes[symbol] = (code, length)
    codes: [(u16, u8); 256],
}

/// Build a JPEG-legal optimal Huffman table from symbol frequencies
/// (the T.81 Annex K.2 procedure: reserve one dummy symbol so no real
/// symbol is assigned the all-ones code, limit lengths to 16).
fn build_huff_table(freq_in: &[u32; 256]) -> HuffTable {
    let mut freq = [0i64; 257];
    for (i, &f) in freq_in.iter().enumerate() {
        freq[i] = f as i64;
    }
    freq[256] = 1; // reserved: guarantees no all-ones code

    let mut codesize = [0usize; 257];
    let mut others = [-1i32; 257];

    loop {
        // Find the two lowest-frequency nonzero entries (ties -> larger index).
        let (mut v1, mut v2) = (-1i32, -1i32);
        let mut c1 = i64::MAX;
        for (i, &fr) in freq.iter().enumerate() {
            if fr > 0 && fr <= c1 {
                c1 = fr;
                v1 = i as i32;
            }
        }
        let mut c2 = i64::MAX;
        for (i, &fr) in freq.iter().enumerate() {
            if fr > 0 && fr <= c2 && i as i32 != v1 {
                c2 = fr;
                v2 = i as i32;
            }
        }
        if v2 < 0 {
            break; // one tree left — done
        }
        freq[v1 as usize] += freq[v2 as usize];
        freq[v2 as usize] = 0;

        codesize[v1 as usize] += 1;
        let mut i = v1 as usize;
        while others[i] >= 0 {
            i = others[i] as usize;
            codesize[i] += 1;
        }
        others[i] = v2;
        codesize[v2 as usize] += 1;
        let mut i = v2 as usize;
        while others[i] >= 0 {
            i = others[i] as usize;
            codesize[i] += 1;
        }
    }

    let mut bits_long = [0u8; 33];
    for &cs in codesize.iter() {
        if cs > 0 {
            bits_long[cs.min(32)] += 1;
        }
    }
    // Limit code lengths to 16 (Annex K.2, Adjust_BITS).
    for i in (17..=32).rev() {
        while bits_long[i] > 0 {
            let mut j = i - 2;
            while bits_long[j] == 0 {
                j -= 1;
            }
            bits_long[i] -= 2;
            bits_long[i - 1] += 1;
            bits_long[j + 1] += 2;
            bits_long[j] -= 1;
        }
    }
    // Remove the reserved dummy symbol from the longest used length.
    let mut i = 16;
    while bits_long[i] == 0 {
        i -= 1;
    }
    bits_long[i] -= 1;

    let mut bits = [0u8; 17];
    bits[1..=16].copy_from_slice(&bits_long[1..=16]);

    // Symbol values sorted by code length, then by symbol value.
    let mut values = Vec::new();
    for len in 1..=32 {
        for (sym, &cs) in codesize.iter().enumerate().take(256) {
            if cs == len {
                values.push(sym as u8);
            }
        }
    }

    // Canonical code assignment.
    let mut codes = [(0u16, 0u8); 256];
    let mut code = 0u16;
    let mut vi = 0usize;
    for len in 1..=16u8 {
        for _ in 0..bits[len as usize] {
            codes[values[vi] as usize] = (code, len);
            code += 1;
            vi += 1;
        }
        code <<= 1;
    }

    HuffTable {
        bits,
        values,
        codes,
    }
}

// ---------------------------------------------------------------- bit writer

struct BitWriter {
    out: Vec<u8>,
    /// Pending bits, right-aligned; fewer than 32 between calls.
    acc: u64,
    nbits: u32,
}

impl BitWriter {
    fn new(out: Vec<u8>) -> Self {
        Self {
            out,
            acc: 0,
            nbits: 0,
        }
    }

    /// Append the low `n` (<= 16) bits of `bits`. Whole 32-bit words are
    /// written at once; only a word that contains 0xFF takes the per-byte
    /// stuffing path.
    #[inline]
    fn put(&mut self, bits: u16, n: u8) {
        self.acc = (self.acc << n) | bits as u64;
        self.nbits += n as u32;
        if self.nbits >= 32 {
            self.nbits -= 32;
            let word = (self.acc >> self.nbits) as u32;
            self.acc &= (1u64 << self.nbits) - 1;
            let bytes = word.to_be_bytes();
            // A byte is 0xFF iff it has no zero bit: test all four at once.
            if (!word).wrapping_sub(0x0101_0101) & word & 0x8080_8080 == 0 {
                self.out.extend_from_slice(&bytes);
            } else {
                for b in bytes {
                    self.out.push(b);
                    if b == 0xFF {
                        self.out.push(0x00); // byte stuffing
                    }
                }
            }
        }
    }

    fn flush(&mut self) {
        // Drain whole bytes first, then pad the last partial byte with ones.
        while self.nbits >= 8 {
            self.nbits -= 8;
            let b = (self.acc >> self.nbits) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0x00);
            }
        }
        self.acc &= (1u64 << self.nbits) - 1;
        if self.nbits > 0 {
            let pad = 8 - self.nbits;
            let b = ((self.acc << pad) | ((1 << pad) - 1)) as u8;
            self.out.push(b);
            if b == 0xFF {
                self.out.push(0x00);
            }
            self.acc = 0;
            self.nbits = 0;
        }
    }
}

// ---------------------------------------------------------------- document

#[allow(clippy::too_many_arguments)]
fn encode_document(
    mut comps: Vec<Component>,
    order: &[(usize, usize)],
    qts: &[[u16; 64]],
    width: u16,
    height: u16,
    app_markers: &[u8],
    lambda: f32,
    chroma_lambda_scale: f32,
    freq_gamma: f32,
    progressive: bool,
    lambda_gate: bool,
    deadzone: (f32, f32),
    aq_lite: (f32, f32, f32, f32),
    jfif: bool,
) -> Vec<u8> {
    // Trellis distortion weights per quant table (zigzag order); gamma 0
    // leaves every weight at exactly 1.0, i.e. plain step units.
    let freq_wts: Vec<[f32; 64]> = qts
        .iter()
        .map(|qt| {
            let mut wv = [1f32; 64];
            if freq_gamma != 0.0 {
                let gm = (qt.iter().skip(1).map(|&q| (q as f32).ln()).sum::<f32>() / 63.0).exp();
                for k in 1..64 {
                    wv[k] = (qt[ZIGZAG[k]] as f32 / gm).powf(freq_gamma);
                }
            }
            wv
        })
        .collect();
    // Pass 1: symbol statistics from plain rounding.
    let n_tables = comps.iter().map(|c| c.table_class).max().unwrap_or(0) * 2 + 2;
    let mut freqs = vec![[0u32; 256]; n_tables];
    for_each_symbol(&comps, order, |tbl, sym, _n, _bits| {
        freqs[tbl][sym as usize] += 1;
    });

    // Trellis pass: re-decide every AC coefficient against rate estimates
    // derived from pass 1, then redo the statistics for the final tables.
    let ac_bits: Vec<[f32; 256]> = (0..n_tables / 2)
        .map(|tc| estimate_bits(&freqs[tc * 2 + 1]))
        .collect();
    // Visual-masking multipliers per luma block: activity = pixel-domain
    // AC energy; noise hides in texture, so busy blocks get a larger
    // lambda (one-sided — flat blocks keep the base).
    let (dz_alpha, dz_cap) = deadzone;
    let mults: Vec<Vec<f32>> = comps
        .iter()
        .map(|c| {
            if dz_alpha <= 0.0 || c.table_class != 0 {
                return Vec::new();
            }
            let qt = &qts[c.qt_id as usize];
            let mut logs = vec![0f32; c.raw.len()];
            let mut sum = 0f64;
            for (bi, raw) in c.raw.iter().enumerate() {
                let mut a = 0f32;
                for k in 1..64 {
                    let e = raw[k] * qt[ZIGZAG[k]] as f32;
                    a += e * e;
                }
                logs[bi] = (a + 1.0).ln();
                sum += logs[bi] as f64;
            }
            let mbar = (sum / logs.len() as f64) as f32;
            logs.iter()
                .map(|&m| (dz_alpha * (m - mbar)).exp().clamp(1.0, dz_cap))
                .collect()
        })
        .collect();
    // AQ-lite risk map: flat blocks bordering strong-edge blocks (and the
    // edge blocks themselves) get a reduced lambda.
    let (aq_flat, aq_strong, aq_gain, aq_boost) = aq_lite;
    let aq_mults: Vec<Vec<f32>> = comps
        .iter()
        .map(|c| {
            if aq_gain >= 1.0 || c.table_class != 0 {
                return Vec::new();
            }
            let qt = &qts[c.qt_id as usize];
            let n = c.blocks.len();
            let bh = n / c.bw;
            let mut var = vec![0f32; n];
            for (bi, raw) in c.raw.iter().enumerate() {
                let mut a = 0f32;
                for k in 1..64 {
                    let e = raw[k] * qt[ZIGZAG[k]] as f32;
                    a += e * e;
                }
                var[bi] = a / 64.0;
            }
            let mut m = vec![aq_boost; n];
            for by in 0..bh {
                for bx in 0..c.bw {
                    let bi = by * c.bw + bx;
                    let me_flat = var[bi] < aq_flat;
                    let me_strong = var[bi] > aq_strong;
                    if !me_flat && !me_strong {
                        continue;
                    }
                    let mut hit = false;
                    for dy in -1i32..=1 {
                        for dx in -1i32..=1 {
                            if dy == 0 && dx == 0 {
                                continue;
                            }
                            let (ny, nx) = (by as i32 + dy, bx as i32 + dx);
                            if ny < 0 || nx < 0 || ny >= bh as i32 || nx >= c.bw as i32 {
                                continue;
                            }
                            let nv = var[(ny as usize) * c.bw + nx as usize];
                            if (me_flat && nv > aq_strong) || (me_strong && nv < aq_flat) {
                                hit = true;
                            }
                        }
                    }
                    if hit {
                        m[bi] = aq_gain;
                    }
                }
            }
            m
        })
        .collect();

    let run_trellis = |comps: &mut Vec<Component>, mult: f32| {
        for (ci, c) in comps.iter_mut().enumerate() {
            let bits = &ac_bits[c.table_class];
            let wt = &freq_wts[c.qt_id as usize];
            let lam = lambda
                * mult
                * if c.table_class == 1 {
                    chroma_lambda_scale
                } else {
                    1.0
                };
            let m = &mults[ci];
            #[cfg(feature = "parallel")]
            {
                // Blocks are independent, so chunked scoped threads produce
                // byte-identical output in any interleaving.
                let nthreads = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(1)
                    .min(8);
                let n = c.blocks.len();
                let chunk = n.div_ceil(nthreads.max(1));
                let raw = &c.raw;
                std::thread::scope(|sc| {
                    for (t, (bch, rch)) in c
                        .blocks
                        .chunks_mut(chunk)
                        .zip(raw.chunks(chunk))
                        .enumerate()
                    {
                        let base = t * chunk;
                        let aqm = &aq_mults[ci];
                        sc.spawn(move || {
                            for (i, (blk, rw)) in bch.iter_mut().zip(rch).enumerate() {
                                let mut lb = if m.is_empty() { lam } else { lam * m[base + i] };
                                if !aqm.is_empty() {
                                    lb *= aqm[base + i];
                                }
                                trellis_ac(rw, blk, bits, lb, wt);
                            }
                        });
                    }
                });
            }
            #[cfg(not(feature = "parallel"))]
            for (bi, (blk, raw)) in c.blocks.iter_mut().zip(&c.raw).enumerate() {
                let mut lb = if m.is_empty() { lam } else { lam * m[bi] };
                if !aq_mults[ci].is_empty() {
                    lb *= aq_mults[ci][bi];
                }
                trellis_ac(raw, blk, bits, lb, wt);
            }
        }
    };

    // Per-image lambda escalation for gradient-like content. Under the
    // fine-grained tables the default lambda is measurably too
    // conservative on smooth gradients (a 2x lambda sits on a strictly
    // better equal-PSNR curve there), while on noise and photos it only
    // trades quality away. The two are separated by where the AC energy
    // lives: gradients concentrate it in the first couple of zigzag
    // positions, noise spreads it across the spectrum.
    let (mut nz, mut e_low, mut e_all) = (0usize, 0f64, 0f64);
    let mut nblocks = 0usize;
    for c in &comps {
        nblocks += c.blocks.len();
        for (blk, raw) in c.blocks.iter().zip(&c.raw) {
            nz += blk[1..].iter().filter(|&&v| v != 0).count();
            for k in 1..64 {
                let e = (raw[k] * raw[k]) as f64;
                e_all += e;
                if k <= 2 {
                    e_low += e;
                }
            }
        }
    }
    let gradient_like =
        lambda_gate && nz as f64 / nblocks as f64 <= 6.0 && e_all > 0.0 && e_low / e_all >= 0.75;
    run_trellis(&mut comps, 1.0);
    if gradient_like {
        // Try 2x; accept if the true pixel-domain AC error stays within
        // ~0.3 dB (the equal-PSNR win on gradient content was validated
        // curve-level with a lambda sweep). Keep a copy of the
        // 1x coefficients so rejection is a memcpy, not a third trellis.
        let dist = |comps: &Vec<Component>| -> f64 {
            let mut d = 0f64;
            for c in comps.iter() {
                let qt = &qts[c.qt_id as usize];
                for (blk, raw) in c.blocks.iter().zip(&c.raw) {
                    for k in 1..64 {
                        let e = (raw[k] - blk[k] as f32) as f64 * qt[ZIGZAG[k]] as f64;
                        d += e * e;
                    }
                }
            }
            d
        };
        let d_base = dist(&comps);
        let saved: Vec<Vec<[i16; 64]>> = comps.iter().map(|c| c.blocks.clone()).collect();
        run_trellis(&mut comps, 2.0);
        let d2 = dist(&comps);
        // 0.3 dB allowance: 10^(0.03) ~= 1.0715.
        if d2 > d_base.max(f64::MIN_POSITIVE) * 1.0715 {
            for (c, blocks) in comps.iter_mut().zip(saved) {
                c.blocks = blocks;
            }
        }
    }
    // DC trellis: the DC coefficients form per-component DPCM chains, so
    // the rate of block i depends on block i-1's choice — a Viterbi pass
    // over candidates {round-1, round, round+1} per block. Rate uses the
    // pass-1 DC statistics; distortion stays in quantization-step units,
    // consistent with the AC trellis.
    {
        let dc_bits: Vec<[f32; 256]> = (0..n_tables / 2)
            .map(|tc| estimate_bits(&freqs[tc * 2]))
            .collect();
        for ci in 0..comps.len() {
            let lam = lambda
                * if comps[ci].table_class == 1 {
                    chroma_lambda_scale
                } else {
                    1.0
                };
            let bits = &dc_bits[comps[ci].table_class];
            let chain: Vec<usize> = order
                .iter()
                .filter(|&&(c, _)| c == ci)
                .map(|&(_, bi)| bi)
                .collect();
            let rate = |prev: i32, cur: i32| -> f32 {
                let sct = category(cur - prev);
                bits[sct as usize] + sct as f32
            };
            let t0: Vec<f32> = chain.iter().map(|&bi| comps[ci].raw[bi][0]).collect();
            let r0: Vec<i32> = t0.iter().map(|t| t.round() as i32).collect();
            // MCU padding blocks are invisible; their distortion is free.
            let vis: Vec<bool> = chain
                .iter()
                .map(|&bi| bi % comps[ci].bw < comps[ci].aw && bi / comps[ci].bw < comps[ci].ah)
                .collect();
            let cand = |j: usize, k: usize| -> i32 { (r0[j] + k as i32 - 1).clamp(-1023, 1023) };
            let mut back: Vec<[u8; 3]> = vec![[0; 3]; chain.len()];
            let mut cost = [0f32; 3];
            for k in 0..3 {
                let v = cand(0, k);
                let d = t0[0] - v as f32;
                cost[k] = if vis[0] { d * d } else { 0.0 } + lam * rate(0, v);
            }
            for j in 1..chain.len() {
                let mut next = [f32::INFINITY; 3];
                for k in 0..3 {
                    let v = cand(j, k);
                    let d = t0[j] - v as f32;
                    let d2 = if vis[j] { d * d } else { 0.0 };
                    for pk in 0..3 {
                        let c = cost[pk] + d2 + lam * rate(cand(j - 1, pk), v);
                        if c < next[k] {
                            next[k] = c;
                            back[j][k] = pk as u8;
                        }
                    }
                }
                cost = next;
            }
            let mut k = (0..3).min_by(|&a, &b| cost[a].total_cmp(&cost[b])).unwrap();
            for j in (0..chain.len()).rev() {
                comps[ci].blocks[chain[j]][0] = cand(j, k) as i16;
                if j > 0 {
                    k = back[j][k] as usize;
                }
            }
        }
    }

    // The trellis outcome is final; the pre-rounding coefficients are no
    // longer needed, so free them before the entropy stages run.
    for c in comps.iter_mut() {
        c.raw = Vec::new();
    }

    // Headers common to both modes, up to (not including) the SOF marker.
    let mut head = Vec::new();
    head.extend_from_slice(&[0xFF, 0xD8]); // SOI
    // APP0 / JFIF (defined for 1 and 3 components only; four-component
    // files carry an Adobe APP14 in `app_markers` instead, or nothing).
    if jfif {
        head.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x10]);
        head.extend_from_slice(b"JFIF\0");
        head.extend_from_slice(&[0x01, 0x01, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00]);
    }
    // Optional APP1 (EXIF) / APP2 (ICC) segments, pre-rendered.
    head.extend_from_slice(app_markers);
    // DQT
    for (id, qt) in qts.iter().enumerate() {
        head.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43, id as u8]);
        for i in 0..64 {
            head.push(qt[ZIGZAG[i]] as u8);
        }
    }
    // SOF0 (baseline) / SOF2 (progressive); frame parameters are identical.
    let emit_sof = |out: &mut Vec<u8>, marker: u8| {
        let sof_len = 8 + 3 * comps.len() as u16;
        out.extend_from_slice(&[0xFF, marker]);
        out.extend_from_slice(&sof_len.to_be_bytes());
        out.push(8);
        out.extend_from_slice(&height.to_be_bytes());
        out.extend_from_slice(&width.to_be_bytes());
        out.push(comps.len() as u8);
        for (i, c) in comps.iter().enumerate() {
            out.push(i as u8 + 1);
            out.push((c.h << 4) | c.v);
            out.push(c.qt_id);
        }
    };

    // Baseline candidate: one interleaved scan.
    let baseline = {
        let mut out = head.clone();
        emit_sof(&mut out, 0xC0);
        let mut freqs = vec![[0u32; 256]; n_tables];
        for_each_symbol(&comps, order, |tbl, sym, _n, _bits| {
            freqs[tbl][sym as usize] += 1;
        });
        let tables: Vec<HuffTable> = freqs.iter().map(build_huff_table).collect();
        for (id, t) in tables.iter().enumerate() {
            emit_dht(&mut out, (id & 1) as u8, (id / 2) as u8, t);
        }
        let sos_len = 6 + 2 * comps.len() as u16;
        out.extend_from_slice(&[0xFF, 0xDA]);
        out.extend_from_slice(&sos_len.to_be_bytes());
        out.push(comps.len() as u8);
        for (i, c) in comps.iter().enumerate() {
            out.push(i as u8 + 1);
            let dc = c.table_class as u8;
            let ac = c.table_class as u8;
            out.push((dc << 4) | ac);
        }
        out.extend_from_slice(&[0x00, 0x3F, 0x00]);
        let mut writer = BitWriter::new(out);
        for_each_symbol(&comps, order, |tbl, sym, n, bits| {
            let (code, len) = tables[tbl].codes[sym as usize];
            writer.put(code, len);
            writer.put(bits, n);
        });
        writer.flush();
        let mut out = writer.out;
        out.extend_from_slice(&[0xFF, 0xD9]); // EOI
        out
    };
    if !progressive {
        return baseline;
    }

    // Progressive candidate: succeeds on photographic content, can lose on
    // very smooth gradients where refinement passes cost more than EOB
    // runs save. The DCT/trellis work is shared, so rendering both entropy
    // streams and keeping the smaller one is cheap scan optimization.
    let mut prog_head = head;
    emit_sof(&mut prog_head, 0xC2);
    let prog = encode_progressive_scans(&comps, order, prog_head);
    if prog.len() <= baseline.len() {
        prog
    } else {
        baseline
    }
}

fn emit_dht(out: &mut Vec<u8>, class: u8, dest: u8, t: &HuffTable) {
    let len = 2 + 1 + 16 + t.values.len() as u16;
    out.extend_from_slice(&[0xFF, 0xC4]);
    out.extend_from_slice(&len.to_be_bytes());
    out.push((class << 4) | dest);
    out.extend_from_slice(&t.bits[1..=16]);
    out.extend_from_slice(&t.values);
}

enum ScanSpec {
    /// Interleaved DC scan, Ah = 0, point transform `al`.
    DcFirst { al: u8 },
    /// Interleaved DC refinement: one raw bit per block, no Huffman data.
    DcRefine { al: u8 },
    /// Single-component AC band, Ah = 0, point transform `al`.
    AcFirst {
        ci: usize,
        ss: usize,
        se: usize,
        al: u8,
    },
    /// Single-component AC refinement, Ah = al + 1.
    AcRefine {
        ci: usize,
        ss: usize,
        se: usize,
        al: u8,
    },
}

/// Progressive scan sequence with spectral selection AND successive
/// approximation (the common simple progression): coefficients are
/// sent coarse-first, then refined one bit plane at a time. The wins over
/// baseline come from EOB runs pricing empty blocks across the image, the
/// coarse first scans concentrating symbol statistics, and every scan
/// getting its own freshly optimized Huffman table (DHT redefinition
/// between scans is legal and effectively free).
fn encode_progressive_scans(
    comps: &[Component],
    order: &[(usize, usize)],
    mut out: Vec<u8>,
) -> Vec<u8> {
    use ScanSpec::*;

    // Sparse (CSR) views of the AC coefficients: the scan search and the
    // AC scan emission only ever touch nonzero coefficients, which is what
    // makes the progressive stage cheap (byte-identical to the dense path).
    let sparse: Vec<SparseAc> = comps.iter().map(sparse_ac).collect();

    // ---- Scan-script optimization: measure the exact coded size of every
    // candidate (statistics pass + optimal table), pick the cheapest. This
    // is what lets progressive win on smooth content too — no fixed script
    // fits every image.

    // Exact bit cost of one Huffman-coded scan given its symbol stats.
    let table_bits = |freq: &[u32; 256], payload: u64| -> u64 {
        let t = build_huff_table(freq);
        let sym_bits: u64 = freq
            .iter()
            .enumerate()
            .map(|(s, &f)| f as u64 * t.codes[s].1 as u64)
            .sum();
        // DHT (19 + values) and SOS (~12) bytes of header per scan.
        sym_bits + payload + (19 + t.values.len() as u64 + 12) * 8
    };

    // DC: choose the point transform by measured cost. A refinement level
    // costs exactly one raw bit per block plus a scan header.
    let n_dc = comps.iter().map(|c| c.table_class).max().unwrap_or(0) + 1;
    let dc_cost = |al: u8| -> u64 {
        let mut freqs = vec![[0u32; 256]; n_dc];
        let mut payload = 0u64;
        for_each_dc_symbol(comps, order, al, |tbl, sym, n, _| {
            freqs[tbl][sym as usize] += 1;
            payload += n as u64;
        });
        let mut bits: u64 = freqs.iter().map(|f| table_bits(f, 0)).sum();
        bits += payload;
        bits += al as u64 * (order.len() as u64 + 12 * 8); // refinement scans
        bits
    };
    let dc_al = if dc_cost(1) < dc_cost(0) { 1u8 } else { 0 };

    // AC per component: band split {no, yes} x point transform {0, 1, 2}.
    // Refinement cost per level is independent of the chain's top, so it
    // is measured once per level.
    let ac_refine_cost = |ci: usize, al: u8| -> u64 {
        let mut freq = [0u32; 256];
        let mut payload = 0u64;
        for_each_ac_refine_symbol_sparse(&sparse[ci], 1, 63, al, |sym, n, _| {
            if let Some(sym) = sym {
                freq[sym as usize] += 1;
            }
            payload += n as u64;
        });
        table_bits(&freq, payload)
    };

    let mut scans: Vec<ScanSpec> = vec![DcFirst { al: dc_al }];
    let mut chain_al = vec![0u8; comps.len()];
    for ci in 0..comps.len() {
        let refine: [u64; 2] = [ac_refine_cost(ci, 0), ac_refine_cost(ci, 1)];
        // Candidates: no split, or split the band at sp (low 1..=sp, high
        // sp+1..=63). Where the split lands matters a lot: smooth content
        // wants a tiny dense low band (1..=2) so the high band is almost
        // all EOB runs.
        let mut best = (u64::MAX, None::<usize>, 0u8);
        // One sparse pass per band covers all three point transforms.
        let full_s = ac_band_stats_all_al(&sparse[ci], 1, 63);
        let split_s: Vec<_> = [2usize, 5, 8]
            .iter()
            .map(|&sp| {
                (
                    sp,
                    ac_band_stats_all_al(&sparse[ci], 1, sp),
                    ac_band_stats_all_al(&sparse[ci], sp + 1, 63),
                )
            })
            .collect();
        for al in 0u8..=2 {
            let tail: u64 = (0..al).map(|l| refine[l as usize]).sum();
            let (f, pl) = &full_s[al as usize];
            let full = table_bits(f, *pl) + tail;
            if full < best.0 {
                best = (full, None, al);
            }
            for (sp, lo_s, hi_s) in &split_s {
                let (lf, lp) = &lo_s[al as usize];
                let (hf, hp) = &hi_s[al as usize];
                let split = table_bits(lf, *lp) + table_bits(hf, *hp) + tail;
                if split < best.0 {
                    best = (split, Some(*sp), al);
                }
            }
        }
        let (_, split, al) = best;
        chain_al[ci] = al;
        match split {
            Some(sp) => {
                scans.push(AcFirst {
                    ci,
                    ss: 1,
                    se: sp,
                    al,
                });
                scans.push(AcFirst {
                    ci,
                    ss: sp + 1,
                    se: 63,
                    al,
                });
            }
            None => scans.push(AcFirst {
                ci,
                ss: 1,
                se: 63,
                al,
            }),
        }
    }
    // Refinement rounds, coarse planes first; DC refinement rides along.
    let max_al = chain_al.iter().copied().max().unwrap_or(0).max(dc_al);
    for level in (0..max_al).rev() {
        if level < dc_al {
            scans.push(DcRefine { al: level });
        }
        for ci in 0..comps.len() {
            if level < chain_al[ci] {
                scans.push(AcRefine {
                    ci,
                    ss: 1,
                    se: 63,
                    al: level,
                });
            }
        }
    }

    let sos_interleaved = |out: &mut Vec<u8>, ah: u8, al: u8| {
        let sos_len = 6 + 2 * comps.len() as u16;
        out.extend_from_slice(&[0xFF, 0xDA]);
        out.extend_from_slice(&sos_len.to_be_bytes());
        out.push(comps.len() as u8);
        for (i, c) in comps.iter().enumerate() {
            out.push(i as u8 + 1);
            out.push((c.table_class as u8) << 4);
        }
        out.extend_from_slice(&[0x00, 0x00, (ah << 4) | al]);
    };

    for spec in scans {
        match spec {
            DcFirst { al } => {
                let n_dc = comps.iter().map(|c| c.table_class).max().unwrap_or(0) + 1;
                let mut freqs = vec![[0u32; 256]; n_dc];
                for_each_dc_symbol(comps, order, al, |tbl, sym, _n, _bits| {
                    freqs[tbl][sym as usize] += 1;
                });
                let tables: Vec<HuffTable> = freqs.iter().map(build_huff_table).collect();
                for (dest, t) in tables.iter().enumerate() {
                    emit_dht(&mut out, 0, dest as u8, t);
                }
                sos_interleaved(&mut out, 0, al);
                let mut writer = BitWriter::new(out);
                for_each_dc_symbol(comps, order, al, |tbl, sym, n, bits| {
                    let (code, len) = tables[tbl].codes[sym as usize];
                    writer.put(code, len);
                    writer.put(bits, n);
                });
                writer.flush();
                out = writer.out;
            }
            DcRefine { al } => {
                sos_interleaved(&mut out, al + 1, al);
                let mut writer = BitWriter::new(out);
                for &(ci, bi) in order {
                    let bit = ((comps[ci].blocks[bi][0] as i32) >> al) & 1;
                    writer.put(bit as u16, 1);
                }
                writer.flush();
                out = writer.out;
            }
            AcFirst { ci, ss, se, al } => {
                let mut freq = [0u32; 256];
                for_each_ac_band_symbol_sparse(&sparse[ci], ss, se, al, |sym, _n, _bits| {
                    freq[sym as usize] += 1
                });
                let t = build_huff_table(&freq);
                emit_dht(&mut out, 1, 0, &t);
                out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 1, ci as u8 + 1, 0x00]);
                out.extend_from_slice(&[ss as u8, se as u8, al]);
                let mut writer = BitWriter::new(out);
                for_each_ac_band_symbol_sparse(&sparse[ci], ss, se, al, |sym, n, bits| {
                    let (code, len) = t.codes[sym as usize];
                    writer.put(code, len);
                    writer.put(bits, n);
                });
                writer.flush();
                out = writer.out;
            }
            AcRefine { ci, ss, se, al } => {
                let mut freq = [0u32; 256];
                for_each_ac_refine_symbol_sparse(&sparse[ci], ss, se, al, |sym, _n, _bits| {
                    if let Some(sym) = sym {
                        freq[sym as usize] += 1;
                    }
                });
                let t = build_huff_table(&freq);
                emit_dht(&mut out, 1, 0, &t);
                out.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 1, ci as u8 + 1, 0x00]);
                out.extend_from_slice(&[ss as u8, se as u8, ((al + 1) << 4) | al]);
                let mut writer = BitWriter::new(out);
                for_each_ac_refine_symbol_sparse(&sparse[ci], ss, se, al, |sym, n, bits| {
                    if let Some(sym) = sym {
                        let (code, len) = t.codes[sym as usize];
                        writer.put(code, len);
                    }
                    writer.put(bits, n);
                });
                writer.flush();
                out = writer.out;
            }
        }
    }

    out.extend_from_slice(&[0xFF, 0xD9]); // EOI
    out
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn psnr(a: &[u8], b: &[u8]) -> f64 {
        let mse: f64 = a
            .iter()
            .zip(b)
            .map(|(&x, &y)| {
                let d = x as f64 - y as f64;
                d * d
            })
            .sum::<f64>()
            / a.len() as f64;
        10.0 * (255.0f64 * 255.0 / mse).log10()
    }

    fn test_image(w: u32, h: u32) -> Vec<u8> {
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                rgb.push((x * 255 / w) as u8);
                rgb.push((y * 255 / h) as u8);
                rgb.push(((x + y) * 128 / (w + h)) as u8);
            }
        }
        rgb
    }

    #[test]
    fn roundtrip_rgb_444() {
        let (w, h) = (65u32, 41u32); // non-multiple-of-8 on purpose
        let rgb = test_image(w, h);
        let jpg = Encoder::new(90)
            .sampling(Sampling::S444)
            .encode_rgb(&rgb, w, h);
        let img = image::load_from_memory(&jpg).expect("decodable");
        assert_eq!((img.width(), img.height()), (w, h));
        let dec = img.to_rgb8();
        assert!(psnr(&rgb, dec.as_raw()) > 30.0, "psnr too low");
    }

    /// The (id, natural-order table) pairs of every DQT in `jpg`.
    fn dqt_tables(jpg: &[u8]) -> Vec<(u8, [u16; 64])> {
        let mut out = Vec::new();
        let mut i = 2;
        while i + 4 <= jpg.len() && jpg[i] == 0xFF && jpg[i + 1] != 0xDA {
            let len = u16::from_be_bytes([jpg[i + 2], jpg[i + 3]]) as usize;
            if jpg[i + 1] == 0xDB {
                let mut t = [0u16; 64];
                for k in 0..64 {
                    t[ZIGZAG[k]] = jpg[i + 5 + k] as u16;
                }
                out.push((jpg[i + 4], t));
            }
            i += 2 + len;
        }
        out
    }

    #[test]
    fn rgb_trellis_tuning_follows_preset() {
        let photo = Encoder::new(80);
        assert_eq!(
            photo.rgb_trellis_tuning(),
            (CHROMA_SCALE_PHOTO_WEIGHTED, TRELLIS_FREQ_GAMMA_PHOTO)
        );
        let annexk = Encoder::new(80).quant_preset(QuantPreset::AnnexK);
        assert_eq!(annexk.rgb_trellis_tuning(), (CHROMA_SCALE_ANNEXK, 0.0));
        let manual = Encoder::new(80).trellis_chroma_scale(0.25);
        assert_eq!(manual.rgb_trellis_tuning().0, 0.25);
    }

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

    #[test]
    fn photo_444_doubles_chroma_steps() {
        let (w, h) = (64u32, 48u32);
        let rgb = test_image(w, h);
        let tables =
            |q: u8, s: Sampling| dqt_tables(&Encoder::new(q).sampling(s).encode_rgb(&rgb, w, h));
        for q in [50u8, 75, 95] {
            let (t420, t444) = (tables(q, Sampling::S420), tables(q, Sampling::S444));
            assert_eq!(t420[0], t444[0], "luma unchanged at q{q}");
            for k in 0..64 {
                assert_eq!(t444[1].1[k], (t420[1].1[k] * 2).min(255), "q{q} step {k}");
            }
        }
        // Quality 100 keeps all-1 tables; AnnexK is untouched.
        assert!(
            tables(100, Sampling::S444)
                .iter()
                .all(|(_, t)| t.iter().all(|&v| v == 1))
        );
        let ak = |s: Sampling| {
            dqt_tables(
                &Encoder::new(75)
                    .quant_preset(QuantPreset::AnnexK)
                    .sampling(s)
                    .encode_rgb(&rgb, w, h),
            )
        };
        assert_eq!(ak(Sampling::S420), ak(Sampling::S444));
    }

    #[test]
    fn roundtrip_rgb_420() {
        let (w, h) = (130u32, 97u32);
        let rgb = test_image(w, h);
        let jpg = Encoder::new(85).encode_rgb(&rgb, w, h);
        let img = image::load_from_memory(&jpg).expect("decodable");
        assert_eq!((img.width(), img.height()), (w, h));
        let dec = img.to_rgb8();
        assert!(psnr(&rgb, dec.as_raw()) > 30.0, "psnr too low");
    }

    #[test]
    fn roundtrip_gray() {
        let (w, h) = (100u32, 60u32);
        let gray: Vec<u8> = (0..w * h).map(|i| (i % 251) as u8).collect();
        let jpg = Encoder::new(90).encode_gray(&gray, w, h);
        let img = image::load_from_memory(&jpg).expect("decodable");
        assert_eq!((img.width(), img.height()), (w, h));
    }

    /// The AAN transform, descaled through dequant_scale with a unit quant
    /// table, must match a naive orthonormal DCT (the JPEG-standard one).
    #[test]
    fn aan_matches_naive_dct() {
        let mut basis = [[0f32; 8]; 8];
        for (k, row) in basis.iter_mut().enumerate() {
            let a = if k == 0 { (1f32 / 8.0).sqrt() } else { 0.5 };
            for (n, v) in row.iter_mut().enumerate() {
                *v = a * ((2 * n + 1) as f32 * k as f32 * std::f32::consts::PI / 16.0).cos();
            }
        }
        let mut blk = [0f32; 64];
        for (i, v) in blk.iter_mut().enumerate() {
            *v = ((i * 37 + i * i * 11) % 256) as f32 - 128.0;
        }

        let mut naive = [0f32; 64];
        for u in 0..8 {
            for v in 0..8 {
                let mut acc = 0f32;
                for y in 0..8 {
                    for x in 0..8 {
                        acc += blk[y * 8 + x] * basis[u][y] * basis[v][x];
                    }
                }
                naive[u * 8 + v] = acc;
            }
        }

        let mut aan = blk;
        fdct8x8(&mut aan);
        let recip = dequant_scale(&[1u16; 64]);
        for i in 0..64 {
            let got = aan[i] * recip[i];
            assert!(
                (got - naive[i]).abs() < 0.01,
                "coeff {i}: aan {got} vs naive {}",
                naive[i]
            );
        }
    }

    /// The sparse walkers must emit exactly the dense walkers' symbol
    /// streams. Adversarial coefficient soup: empty blocks in long runs
    /// (EOBRUN cap territory), 16+ zero gaps (ZRL boundaries), small
    /// magnitudes that vanish under the point transform, history-significant
    /// vs newly-significant mixes, and a padded block grid (bw > aw).
    #[test]
    fn sparse_walkers_match_dense() {
        let (aw, ah, bw) = (210usize, 160usize, 213usize);
        let mut seed = 0x5eed_cafe_u64;
        let mut rnd = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as u32
        };
        let mut blocks = vec![[0i16; 64]; bw * ah];
        for by in 0..ah {
            for bx in 0..aw {
                let blk = &mut blocks[by * bw + bx];
                match rnd() % 10 {
                    0..=4 => {} // empty (long EOB runs)
                    5 | 6 => {
                        // sparse small values, wide gaps
                        for _ in 0..(1 + rnd() % 3) {
                            let k = 1 + (rnd() % 63) as usize;
                            blk[k] = [1, -1, 2, -2, 3, -3][(rnd() % 6) as usize];
                        }
                    }
                    7 | 8 => {
                        for _ in 0..(4 + rnd() % 12) {
                            let k = 1 + (rnd() % 63) as usize;
                            blk[k] = (rnd() % 4000) as i16 - 2000;
                        }
                    }
                    _ => {
                        // tail-heavy: nonzero at se and beyond ZRL gaps
                        blk[63] = 5;
                        blk[1 + (rnd() % 20) as usize] = -7;
                    }
                }
                // padding columns stay whatever; they must be ignored
            }
        }
        let c = Component {
            blocks,
            raw: Vec::new(),
            table_class: 0,
            bw,
            aw,
            ah,
            h: 1,
            v: 1,
            qt_id: 0,
            e_low: 0.0,
            e_all: 0.0,
            energy: false,
        };
        let s = sparse_ac(&c);
        for &(ss, se) in &[(1usize, 63usize), (1, 5), (6, 63), (1, 2), (3, 63)] {
            for al in 0u8..=2 {
                let mut dense = Vec::new();
                for_each_ac_band_symbol(&c, ss, se, al, |a, b, cc| dense.push((a, b, cc)));
                let mut sp = Vec::new();
                for_each_ac_band_symbol_sparse(&s, ss, se, al, |a, b, cc| sp.push((a, b, cc)));
                assert_eq!(dense, sp, "band ss={ss} se={se} al={al}");
            }
            for al in 0u8..=1 {
                let mut dense = Vec::new();
                for_each_ac_refine_symbol(&c, ss, se, al, |a, b, cc| dense.push((a, b, cc)));
                let mut sp = Vec::new();
                for_each_ac_refine_symbol_sparse(&s, ss, se, al, |a, b, cc| sp.push((a, b, cc)));
                assert_eq!(dense, sp, "refine ss={ss} se={se} al={al}");
            }
        }
        // fused stats must equal three separate passes
        for &(ss, se) in &[(1usize, 63usize), (1, 8), (9, 63)] {
            let all = ac_band_stats_all_al(&s, ss, se);
            for al in 0u8..=2 {
                let mut freq = [0u32; 256];
                let mut payload = 0u64;
                for_each_ac_band_symbol_sparse(&s, ss, se, al, |sym, n, _| {
                    freq[sym as usize] += 1;
                    payload += n as u64;
                });
                assert_eq!(all[al as usize].0, freq, "fused freq al={al}");
                assert_eq!(all[al as usize].1, payload, "fused payload al={al}");
            }
        }
    }

    #[test]
    fn flat_image_is_tiny() {
        let rgb = vec![128u8; 64 * 64 * 3];
        let jpg = Encoder::new(80).encode_rgb(&rgb, 64, 64);
        assert!(
            jpg.len() < 1500,
            "flat image should compress hard: {}",
            jpg.len()
        );
    }
}

#[cfg(test)]
mod meta_tests {
    use super::*;

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

    fn test_rgb(w: u32, h: u32) -> Vec<u8> {
        (0..w * h)
            .flat_map(|i| {
                let x = (i % w) * 255 / w;
                let y = (i / w) * 255 / h;
                [x as u8, y as u8, 128]
            })
            .collect()
    }

    #[test]
    fn icc_and_exif_are_embedded_and_survive_optimize() {
        let rgb = test_rgb(40, 30);
        let icc: Vec<u8> = (0..1000u32).map(|i| i as u8).collect();
        let exif: Vec<u8> = b"II*\0test-tiff-stream".to_vec();
        let j = Encoder::new(80)
            .icc_profile(&icc)
            .exif(&exif)
            .encode_rgb(&rgb, 40, 30);
        let segs = segments(&j);
        let app1: Vec<_> = segs.iter().filter(|s| s.0 == 0xE1).collect();
        assert_eq!(app1.len(), 1);
        assert_eq!(&app1[0].1[..6], b"Exif\0\0");
        assert_eq!(&app1[0].1[6..], &exif[..]);
        let app2: Vec<_> = segs.iter().filter(|s| s.0 == 0xE2).collect();
        assert_eq!(app2.len(), 1);
        assert_eq!(&app2[0].1[..12], b"ICC_PROFILE\0");
        assert_eq!(&app2[0].1[12..14], &[1, 1]);
        assert_eq!(&app2[0].1[14..], &icc[..]);
        // Pixels are unaffected by metadata.
        let plain = Encoder::new(80).encode_rgb(&rgb, 40, 30);
        let a = image::load_from_memory(&j).unwrap().to_rgb8();
        let b = image::load_from_memory(&plain).unwrap().to_rgb8();
        assert_eq!(a.into_raw(), b.into_raw());
        // And the lossless optimizer passes them through verbatim.
        let opt = optimize_jpeg(&j).unwrap();
        let osegs = segments(&opt);
        assert!(osegs.iter().any(|s| s.0 == 0xE1 && s.1[6..] == exif[..]));
        assert!(osegs.iter().any(|s| s.0 == 0xE2 && s.1[14..] == icc[..]));
    }

    #[test]
    fn large_icc_is_chunked() {
        let rgb = test_rgb(16, 16);
        let icc: Vec<u8> = (0..150_000u32).map(|i| (i * 7) as u8).collect();
        let j = Encoder::new(80).icc_profile(&icc).encode_rgb(&rgb, 16, 16);
        let segs = segments(&j);
        let chunks: Vec<_> = segs.iter().filter(|s| s.0 == 0xE2).collect();
        assert_eq!(chunks.len(), 3);
        let mut got = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            assert_eq!(&c.1[..12], b"ICC_PROFILE\0");
            assert_eq!(c.1[12] as usize, i + 1);
            assert_eq!(c.1[13] as usize, 3);
            got.extend_from_slice(&c.1[14..]);
        }
        assert_eq!(got, icc);
        assert!(image::load_from_memory(&j).is_ok());
    }

    #[test]
    fn oversized_exif_is_skipped() {
        let rgb = test_rgb(16, 16);
        let big = vec![0u8; 70_000];
        let j = Encoder::new(80).exif(&big).encode_gray(
            &rgb.chunks(3).map(|p| p[0]).collect::<Vec<_>>(),
            16,
            16,
        );
        assert!(!segments(&j).iter().any(|s| s.0 == 0xE1));
        assert!(image::load_from_memory(&j).is_ok());
    }

    /// Every Huffman symbol a file defines must be codable in baseline
    /// 8-bit JPEG: DC sizes up to 11, AC sizes up to 10 (T.81 F.1.2).
    /// Strict decoders (macOS ImageIO) reject the whole file otherwise.
    fn assert_symbols_in_range(j: &[u8], what: &str) {
        for (m, p) in segments(j) {
            if m != 0xC4 {
                continue;
            }
            let mut i = 0;
            while i + 17 <= p.len() {
                let class = p[i] >> 4;
                let n: usize = p[i + 1..i + 17].iter().map(|&c| c as usize).sum();
                for &sym in &p[i + 17..i + 17 + n] {
                    if class == 0 {
                        assert!(sym <= 11, "{what}: DC size {sym}");
                    } else {
                        assert!(sym & 0x0F <= 10, "{what}: AC symbol {sym:#04x}");
                    }
                }
                i += 17 + n;
            }
        }
    }

    #[test]
    fn hard_edges_at_high_quality_stay_baseline_codable() {
        // A QR-like pattern: 3-pixel black/white modules, the worst case
        // for coefficient magnitude (and for deringing overshoot).
        let (w, h) = (111u32, 111u32);
        let gray: Vec<u8> = (0..w * h)
            .map(|i| {
                let (x, y) = ((i % w) / 3, (i / w) / 3);
                let v = (x.wrapping_mul(2654435761) ^ y.wrapping_mul(40503)) >> 7;
                if v & 1 == 1 { 0 } else { 255 }
            })
            .collect();
        let rgb: Vec<u8> = gray.iter().flat_map(|&v| [v, v, v]).collect();
        let cmyk: Vec<u8> = gray.iter().flat_map(|&v| [0, 0, 0, 255 - v]).collect();
        for q in [90u8, 93, 95, 97, 100] {
            for preset in [QuantPreset::AnnexK, QuantPreset::Photo] {
                for prog in [false, true] {
                    let mk = || Encoder::new(q).quant_preset(preset).progressive(prog);
                    let tag = format!("q{q} {preset:?} prog={prog}");
                    let g = mk().encode_gray(&gray, w, h);
                    assert_symbols_in_range(&g, &format!("gray {tag}"));
                    assert!(optimize_jpeg(&g).is_some(), "gray {tag}");
                    let c = mk().sampling(Sampling::S444).encode_rgb(&rgb, w, h);
                    assert_symbols_in_range(&c, &format!("rgb {tag}"));
                    assert!(optimize_jpeg(&c).is_some(), "rgb {tag}");
                    for adobe in [None, Some(AdobeTransform::Ycck)] {
                        let k = mk().encode_cmyk(&cmyk, w, h, adobe);
                        assert_symbols_in_range(&k, &format!("cmyk {adobe:?} {tag}"));
                        assert!(decode_cmyk(&k).is_some(), "cmyk {adobe:?} {tag}");
                    }
                }
            }
        }
    }
}
