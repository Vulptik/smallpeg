# JavaScript API

The npm package `smallpeg`. TypeScript types are included, and every
function's documentation shows up in editors.

## Entry points

| Import | For |
|---|---|
| `"smallpeg"` | Node, and browsers through a bundler (resolves to the browser build there) |
| `"smallpeg/web"` | Browsers and bundlers, explicitly |

Both export the same functions. The browser build has to load its `.wasm`
file before use:

```js
import { init, initSync, encode } from "smallpeg/web";

await init();            // fetches web/smallpeg_wasm_bg.wasm from the package
await init(url);         // or a URL, a Response, a fetch() promise, bytes, a WebAssembly.Module
initSync(bytes);         // synchronously, from bytes or a WebAssembly.Module (workers)
```

`init` can be called again harmlessly; a failed load can be retried.
Calling another function before the module has loaded throws an `Error`.

In Node the module loads on import. `init` and `initSync` exist there too
and do nothing, so code shared between Node and the browser can always
call them.

`require("smallpeg")` works in Node 22.12 or later. The package is ES
modules, so TypeScript code compiled to CommonJS needs `"module":
"nodenext"` and TypeScript 5.8 or later.

## Images

Pixels are passed and returned as plain objects:

```ts
interface Image {
  data: Uint8Array;   // width * height * channels bytes, interleaved, top row first
  width: number;
  height: number;
  channels: 1 | 3 | 4; // gray, RGB, RGBA (or any four-channel layout)
}
```

When you pass an image in, `data` can also be a `Uint8ClampedArray`, so a
canvas `ImageData` fits as is, and `channels` can be left out: it is worked
out from the length of `data`.

Returned bytes are fresh copies backed by their own `ArrayBuffer`, so they
can go straight into a `Blob`, and 4-channel pixels can be viewed as the
`Uint8ClampedArray` that `new ImageData` takes.

## `encode(image, options?)`

Encode pixels as a JPEG. 4 channels are RGBA: alpha is flattened onto
white.

- `quality`: integer 1 to 100, default 80.
- `fullColor`: keep chroma at full resolution (4:4:4) instead of halving
  it (4:2:0). Sharper colored text and lines, larger files. Default false.

## `optimize(jpeg, options?)`

Re-code an existing JPEG with identical pixels. Returns `undefined` if the
file is not one that can be re-coded. An already optimized file can come
out larger; compare lengths.

- `baseline`: write baseline sequential output instead of progressive,
  for targets that do not accept progressive JPEGs (PDF embedding, for
  example). Default false.

## `stripMetadata(jpeg)`

Remove EXIF (including GPS position), XMP, comments and other metadata
without touching the image data. Keeps the color profile and orientation.
Returns `undefined` if the file cannot be parsed.

## `info(jpeg)`

Read the header without decoding: `{ width, height, components,
progressive, orientation }`, or `undefined` if the bytes are not a JPEG
this library understands. `orientation` is the EXIF orientation, 1 to 8
(1 when the file has none).

## `decode(jpeg)`

Decode a grayscale or color JPEG to 1-channel gray or 3-channel RGB
pixels, as stored. Returns `undefined` for CMYK files, codings this
library does not decode, malformed input, or frames over 100 megapixels.

## `applyOrientation(image, orientation)`

Turn pixels upright for an EXIF orientation (`info(jpeg).orientation`).
Orientations 5 to 8 swap width and height; a number outside 1 to 8 leaves
the pixels as they are.

## `resize(image, width, height, options?)`

Resize with a Lanczos3 filter. 4 channels are filtered independently.
Results over 100 megapixels are refused.

- `linear`: filter in linear light, which keeps fine light and dark
  detail from darkening when reducing. Assumes sRGB. With 4 channels it
  throws an `Error`.

## `crop(image, { x, y, width, height })`

Copy a rectangle out of an image. Returns `undefined` if the rectangle is
empty or not entirely inside the image.

## `fitWithin(width, height, maxWidth, maxHeight)`

The largest `[width, height]` that fits inside `maxWidth` x `maxHeight`
with the same aspect ratio. Never enlarges.

## Errors

- Functions that read JPEG bytes accept untrusted input and return
  `undefined` for anything they cannot handle.
- Arguments of the wrong kind throw a `TypeError`: JPEG bytes must be a
  `Uint8Array` (a Node `Buffer` is one), pixel data a `Uint8Array` or
  `Uint8ClampedArray`, options an object, flags booleans.
- Values out of range throw a `RangeError`: sizes, coordinates and
  `quality` must be integers in range, and `data` must have exactly
  `width * height * channels` bytes.
- Two limits of the format and filter throw a plain `Error`: `encode` with
  a width or height above 65535, and `resize` with `linear` on 4 channels.
- Messages start with `smallpeg:`.

## Example: shrink a photo for upload

```js
import { decode, info, applyOrientation, fitWithin, resize, encode } from "smallpeg";

function shrink(jpeg, maxSide = 1600) {
  const img = decode(jpeg);
  if (!img) return undefined;
  const upright = applyOrientation(img, info(jpeg).orientation);
  const [w, h] = fitWithin(upright.width, upright.height, maxSide, maxSide);
  return encode(resize(upright, w, h, { linear: true }), { quality: 80 });
}
```
