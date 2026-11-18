# smallpeg

[![crates.io](https://img.shields.io/crates/v/smallpeg.svg)](https://crates.io/crates/smallpeg)
[![docs.rs](https://img.shields.io/docsrs/smallpeg)](https://docs.rs/smallpeg)
[![npm](https://img.shields.io/npm/v/smallpeg.svg)](https://www.npmjs.com/package/smallpeg)
[![CI](https://github.com/sawapi/smallpeg/actions/workflows/ci.yml/badge.svg)](https://github.com/sawapi/smallpeg/actions/workflows/ci.yml)
[![license](https://img.shields.io/crates/l/smallpeg.svg)](#license)

A JPEG encoder, lossless optimizer and decoder in pure Rust, written from
the JPEG standard (ITU-T T.81) with no dependencies. The same code runs as
WebAssembly in Node and the browser.

[Website](https://sawapi.github.io/smallpeg/) ·
[API docs](https://docs.rs/smallpeg) ·
[npm package](https://www.npmjs.com/package/smallpeg) ·
[Changelog](https://github.com/sawapi/smallpeg/blob/main/CHANGELOG.md)

## Install

```sh
cargo add smallpeg      # Rust
npm install smallpeg    # Node and the browser
```

The npm package has its own [README](https://github.com/sawapi/smallpeg/blob/main/npm/README.md)
with JavaScript examples.

## Example

```rust
// Encode RGB pixels (width * height * 3 bytes) at quality 80.
let jpeg: Vec<u8> = smallpeg::Encoder::new(80).encode_rgb(&rgb, width, height);

// Make an existing JPEG smaller without changing a pixel.
if let Some(smaller) = smallpeg::optimize_jpeg(&input) {
    if smaller.len() < input.len() {
        std::fs::write("photo.jpg", smaller)?;
    }
}
```

## Features

- **Encode** grayscale, RGB and CMYK, baseline or progressive, with
  trellis quantization and per-image Huffman tables.
- **Optimize** an existing JPEG losslessly: same pixels, fewer bytes.
- **Turn upright** a phone photo by its EXIF orientation, losslessly.
- **Strip metadata** such as EXIF and GPS position, keeping the color
  profile and orientation.
- **Decode**, read the header, fix EXIF orientation, crop and resize
  (Lanczos3, optionally in linear light).
- **Handle untrusted input**: the decoders and the optimizer return
  `None` instead of panicking, with limits checked before anything is
  allocated.
- **Build anywhere**: no dependencies, no `unsafe`, and it compiles for
  `wasm32-unknown-unknown` as is.

## Documentation

- [Guide](https://github.com/sawapi/smallpeg/blob/main/docs/guide.md):
  resizing, cropping, orientation, optimizing, CMYK, untrusted input
- [Color profiles](https://github.com/sawapi/smallpeg/blob/main/docs/color-profiles.md):
  keeping colors right for Display P3 and other non-sRGB sources
- [How the encoder works](https://github.com/sawapi/smallpeg/blob/main/docs/encoder.md):
  trellis quantization, scan search, quantization presets, deringing
- [API reference](https://docs.rs/smallpeg) on docs.rs; the
  [JavaScript API](https://github.com/sawapi/smallpeg/blob/main/docs/npm-api.md)
- [Provenance](https://github.com/sawapi/smallpeg/blob/main/THIRD_PARTY.md):
  sources of the techniques and the quantization table

## Requirements

Rust 1.88 or later. The npm package needs Node 22.12 or later, or a
current browser (Safari 16.4 or later).

## License

Licensed under either of
[Apache License, Version 2.0](https://github.com/sawapi/smallpeg/blob/main/LICENSE-APACHE)
or [MIT license](https://github.com/sawapi/smallpeg/blob/main/LICENSE-MIT)
at your option.
