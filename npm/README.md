# smallpeg

A JPEG encoder, lossless optimizer and decoder written from the JPEG
standard in Rust, compiled to WebAssembly for Node and the browser. No
dependencies and no native binaries: about 190 KB of WebAssembly plus a
small TypeScript-typed API.

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

// data: width * height * channels bytes (1 gray, 3 RGB, 4 RGBA)
const jpeg = encode({ data: rgb, width, height }, { quality: 80 });

// same pixels, fewer bytes (compare lengths: an optimized file may not shrink)
const input = readFileSync("photo.jpg");
const smaller = optimize(input);
if (smaller && smaller.length < input.length) writeFileSync("photo.jpg", smaller);

// remove EXIF, GPS and XMP
const clean = stripMetadata(input);
```

`require("smallpeg")` works too (Node 22.12 or later). The package is ES
modules, so TypeScript code compiled to CommonJS needs `"module":
"nodenext"` and TypeScript 5.8 or later to import it.

## Browser and bundlers

The browser build has to load its `.wasm` before use:

```js
import { init, encode } from "smallpeg/web";

await init(); // fetches web/smallpeg_wasm_bg.wasm from the package
const jpeg = encode(ctx.getImageData(0, 0, w, h)); // RGBA, alpha onto white
const blob = new Blob([jpeg], { type: "image/jpeg" });
```

Bundlers that resolve `"smallpeg"` for the browser get the same build, so
`init()` is needed there as well; in Node it is a no-op, which lets shared
code always call it. Run encoding in a Web Worker for large images: it is
synchronous.

## API

Images are plain objects, `{ data, width, height, channels }`: `data` holds
`width * height * channels` bytes, interleaved, top row first. A canvas
`ImageData` fits as is. `channels` is 1 (gray), 3 (RGB) or 4 (RGBA); when
you pass an image in, it can be left out and is worked out from the length
of `data`. Images that come back can be passed straight to the next call.

| Function | Returns |
|---|---|
| `encode(image, { quality = 80, fullColor = false })` | JPEG bytes. 4 channels are RGBA, flattened onto white. `fullColor` keeps chroma at full resolution (4:4:4) |
| `optimize(jpeg, { baseline = false })` | Re-coded JPEG with identical pixels, or `undefined` if the file cannot be re-coded |
| `stripMetadata(jpeg)` | JPEG without metadata, or `undefined` |
| `info(jpeg)` | `{ width, height, components, progressive, orientation }`, or `undefined` |
| `decode(jpeg)` | Image (gray or RGB, as stored), or `undefined` |
| `applyOrientation(image, orientation)` | Upright image; use `info(jpeg).orientation` |
| `resize(image, width, height, { linear = false })` | Resized image (Lanczos3). `linear` filters in linear light |
| `crop(image, { x, y, width, height })` | Cropped image, or `undefined` if the rectangle is not inside the image |
| `fitWithin(width, height, maxWidth, maxHeight)` | `[width, height]` that fits, keeping the aspect ratio; never enlarges |

Functions that read JPEG bytes accept untrusted input and return
`undefined` for anything they cannot handle. Pixel data whose length does
not match its dimensions, and sizes or coordinates that are not whole
numbers, throw an `Error`. Returned bytes are fresh copies backed by an
`ArrayBuffer`, so they can go straight into a `Blob`. TypeScript types are
included (TypeScript 5.7 or later).

## License

MIT OR Apache-2.0, at your option. The Rust crate is
[`smallpeg`](https://crates.io/crates/smallpeg).
