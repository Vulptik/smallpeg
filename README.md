# smallpeg

A JPEG encoder, lossless optimizer and decoder written from the JPEG
standard (ITU-T T.81) in pure Rust, with no dependencies. It builds for
`wasm32-unknown-unknown` as is, and the same code ships on npm for Node
and the browser.

## Install

```sh
cargo add smallpeg      # Rust
npm install smallpeg    # Node and the browser (see npm/README.md)
```

## Features

- **Encoder**: grayscale, RGB and CMYK; 4:4:4 and 4:2:0 chroma; baseline
  or progressive output, with the scan script optimized per image and the
  smaller of progressive and baseline kept.
- **Trellis quantization** decides each coefficient against its actual
  bit cost, and **per-image Huffman tables** are always on.
- **Two quantization presets**: `Photo` (default) and `AnnexK` (the
  T.81 example tables).
- **Clipped-pixel deringing**: around white text and line art, ringing is
  pushed above 255 where the decoder clamps it away. On rendered
  documents this made files 17–19% smaller at equal SSIMULACRA2.
- **Lossless re-optimization** (`optimize_jpeg`): re-codes an existing
  baseline or progressive JPEG with bit-identical pixels, keeping its
  tables, sampling and metadata. `optimize_jpeg_baseline` always writes
  baseline output, for targets such as PDF.
- **Metadata**: `strip_metadata` removes EXIF (including GPS), XMP,
  comments and trailing data without touching the image, and keeps the
  ICC profile and orientation. `Encoder::icc_profile` and
  `Encoder::exif` attach metadata to new output.
- **Decoding** (`decode_jpeg`, `decode_cmyk`): baseline and progressive,
  any sampling, to 8-bit gray, RGB or stored CMYK values. `jpeg_info`
  reads the header without decoding.
- **Orientation, crop and resize**: `apply_orientation`, `crop`,
  `fit_within`, and Lanczos3 resizing (`resize_rgb`, `resize_gray`,
  `resize_cmyk`), with linear-light variants that keep fine detail from
  darkening when reduced.
- **Untrusted input**: the decoders and `optimize_jpeg` return `None`
  instead of panicking, and `Limits` caps pixels and scans before
  anything is allocated. `optimize_jpeg`, which shares the decoders'
  parsing, is fuzzed weekly in CI.
- **Optional `parallel` feature** (std threads, not for wasm) runs the
  trellis across cores with byte-identical output.

Encoding settings were chosen by measuring file size at equal quality
(PSNR, SSIMULACRA2 and butteraugli) on documents, scans, gradients and
photographs. An A4 300 dpi RGB scan encodes in about 200 ms natively,
with peak memory around 10 bytes per pixel.

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

## License

MIT OR Apache-2.0, at your option.
