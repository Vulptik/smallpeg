# smallpeg

A JPEG encoder, lossless optimizer and decoder written from the JPEG
standard in Rust, compiled to WebAssembly for Node and the browser. No
dependencies and no native binaries: the package is JavaScript glue plus
one `.wasm` file of about 190 KB.

- **Encode** gray, RGB or RGBA pixels (for example from a canvas) with
  trellis quantization, per-image Huffman tables and optimized progressive
  scans.
- **Optimize** an existing JPEG without changing a pixel.
- **Strip metadata** such as EXIF and GPS position, keeping the color
  profile and orientation.
- **Decode**, read the header, fix EXIF orientation, crop and resize
  (Lanczos3, optionally in linear light).

## Node

```js
import { encode, optimize, stripMetadata } from "smallpeg";
import { readFileSync, writeFileSync } from "node:fs";

// rgb: width * height * 3 bytes
const jpeg = encode(rgb, width, height, 3, 80);

// same pixels, fewer bytes (compare lengths: an optimized file may not shrink)
const input = readFileSync("photo.jpg");
const smaller = optimize(input);
if (smaller && smaller.length < input.length) writeFileSync("photo.jpg", smaller);

// remove EXIF, GPS and XMP
const clean = stripMetadata(input);
```

## Browser and bundlers

The browser build has to load its `.wasm` before use:

```js
import init, { encode } from "smallpeg/web";

await init(); // fetches smallpeg_wasm_bg.wasm next to the module
const { data, width, height } = ctx.getImageData(0, 0, w, h);
const jpeg = encode(data, width, height, 4, 80); // RGBA, alpha onto white
const blob = new Blob([jpeg], { type: "image/jpeg" });
```

Run it in a Web Worker for large images: encoding is synchronous.

## API

| Function | Returns |
|---|---|
| `encode(pixels, width, height, channels, quality = 80, fullColor = false)` | JPEG bytes. `channels` is 1, 3 or 4 (RGBA). `fullColor` keeps chroma at full resolution (4:4:4) |
| `optimize(jpeg, baseline = false)` | Re-coded JPEG with identical pixels, or `undefined` if the file cannot be re-coded |
| `stripMetadata(jpeg)` | JPEG without metadata, or `undefined` |
| `info(jpeg)` | `{ width, height, components, progressive, orientation }`, or `undefined` |
| `decode(jpeg)` | `{ width, height, channels, pixels }` (gray or RGB, as stored), or `undefined` |
| `applyOrientation(pixels, width, height, channels, orientation)` | Upright `{ width, height, channels, pixels }` |
| `resize(pixels, width, height, channels, newWidth, newHeight, linear = false)` | Resized pixels |
| `crop(pixels, width, height, channels, x, y, cropWidth, cropHeight)` | Cropped pixels, or `undefined` if the rectangle is not inside the image |
| `fitWithin(width, height, maxWidth, maxHeight)` | `[width, height]` that fits, keeping the aspect ratio; never enlarges |

Functions that read JPEG bytes accept untrusted input and return
`undefined` for anything they cannot handle. A pixel buffer whose length
does not match its dimensions throws an `Error`.

## License

MIT OR Apache-2.0, at your option. The Rust crate is
[`smallpeg`](https://crates.io/crates/smallpeg).
