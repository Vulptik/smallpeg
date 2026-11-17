// Browser (and bundler) entry point: the wasm has to be fetched and
// instantiated with `await init()` (or `initSync()`) before the first call.

import initWasm, * as glue from "../web/smallpeg_wasm.js";
import { type Api, makeApi, type ParamsMatch } from "./core.mjs";

// Fails to compile if the glue's parameters drift from what core expects.
type Assert<T extends { [K in keyof T]: true }> = T;
type _Check = Assert<ParamsMatch<typeof glue>>;

/**
 * Where to load the `.wasm` from: a URL (string or `URL`), a `Request`, a
 * `Response` or a promise of one (e.g. `fetch(url)`), the bytes, or a
 * compiled `WebAssembly.Module`. Typed loosely so the declarations need no
 * DOM types. The form wasm-bindgen documents, `{ module_or_path: ... }`,
 * is accepted too.
 */
export type WasmSource = string | object;

/**
 * What {@link initSync} takes: the bytes or a compiled `WebAssembly.Module`
 * (or wasm-bindgen's `{ module: ... }`).
 */
export type WasmModuleSource = object;

let ready: Promise<void> | undefined;
let loaded = false;

// A plain object carrying `key` is wasm-bindgen's own argument form, which
// 0.13 documented: pass it through. Anything else is the source itself.
function isPlain(x: unknown, key: string): boolean {
  return (
    typeof x === "object" && x !== null && Object.getPrototypeOf(x) === Object.prototype && key in x
  );
}

/**
 * Load the wasm. Call (and await) it once before the other functions;
 * later calls return the same promise, and a failed load can be retried.
 *
 * By default the file is fetched from `web/smallpeg_wasm_bg.wasm` inside
 * the package (bundlers that understand `new URL(..., import.meta.url)`
 * copy it). Pass `wasm` to load it from elsewhere.
 */
export function init(wasm?: WasmSource): Promise<void> {
  // A plain object must be wasm-bindgen's form; `{}` or initSync's
  // `{ module }` here is a mistake, not something to fetch around.
  if (
    typeof wasm === "object" &&
    wasm !== null &&
    Object.getPrototypeOf(wasm) === Object.prototype &&
    !("module_or_path" in wasm)
  ) {
    return Promise.reject(
      new TypeError("smallpeg: init() takes a URL, Response, bytes or WebAssembly.Module"),
    );
  }
  // The casts: WasmSource is deliberately looser than the glue's DOM types.
  const arg =
    wasm === undefined
      ? undefined
      : isPlain(wasm, "module_or_path")
        ? (wasm as never)
        : { module_or_path: wasm as never };
  ready ??= initWasm(arg).then(
    () => {
      loaded = true;
    },
    (err: unknown) => {
      ready = undefined;
      throw err;
    },
  );
  return ready;
}

/**
 * Load the wasm synchronously from bytes you already have (or a compiled
 * `WebAssembly.Module`). Browsers may refuse to compile a module
 * synchronously on the main thread; use it in workers, or prefer
 * {@link init}.
 */
export function initSync(wasm: WasmModuleSource): void {
  if (loaded) return;
  // The glue would instantiate a second copy under the pending load.
  if (ready) throw new Error("smallpeg: init() is still loading; await it instead");
  glue.initSync(isPlain(wasm, "module") ? (wasm as never) : { module: wasm as never });
  loaded = true;
  ready = Promise.resolve();
}

export default init;

const api = makeApi(glue, () => {
  if (!loaded) throw new Error("smallpeg: call `await init()` before using it in the browser");
});

export type {
  Bytes,
  Channels,
  EncodeOptions,
  Image,
  ImageInput,
  JpegInfo,
  OptimizeOptions,
  Orientation,
  Rect,
  ResizeOptions,
} from "./core.mjs";

/**
 * Encode pixels as a JPEG. 4 channels are RGBA: alpha is flattened onto
 * white.
 *
 * @throws {RangeError} if the data length does not match the dimensions,
 * or `quality` is not an integer from 1 to 100.
 * @throws {TypeError} for arguments of the wrong kind.
 * @throws {Error} if a dimension is above 65535.
 */
export const encode: Api["encode"] = api.encode;
/**
 * Make an existing JPEG smaller without changing a pixel. `undefined` if
 * the file is not one this can re-code. An already optimized file can come
 * out larger; compare lengths.
 */
export const optimize: Api["optimize"] = api.optimize;
/**
 * Remove EXIF (including GPS), XMP, comments and other metadata without
 * touching the image data. Keeps the color profile and orientation.
 * `undefined` if the file cannot be parsed.
 */
export const stripMetadata: Api["stripMetadata"] = api.stripMetadata;
/**
 * Read a JPEG's frame header without decoding it. `undefined` if the bytes
 * are not a JPEG this library understands.
 */
export const info: Api["info"] = api.info;
/**
 * Decode a grayscale or color JPEG to 1-channel gray or 3-channel RGB
 * pixels, as stored (see `applyOrientation`). `undefined` for CMYK files,
 * codings this library does not decode, malformed input, or frames over
 * 100 megapixels.
 */
export const decode: Api["decode"] = api.decode;
/**
 * Turn pixels upright for an EXIF orientation (`info(jpeg).orientation`).
 * A number outside 1 to 8 leaves them as they are.
 *
 * @throws {RangeError} if the data length does not match the dimensions.
 * @throws {TypeError} if `orientation` is not a number.
 */
export const applyOrientation: Api["applyOrientation"] = api.applyOrientation;
/**
 * Resize with a Lanczos3 filter. 4 channels are filtered independently.
 *
 * @throws {RangeError} if the data length does not match the dimensions,
 * a new size is not an integer of at least 1, or the result would be over
 * 100 megapixels.
 * @throws {Error} if `linear` is set for 4 channels.
 */
export const resize: Api["resize"] = api.resize;
/**
 * Copy a rectangle out of an image. `undefined` if the rectangle is empty
 * or not entirely inside the image.
 *
 * @throws {RangeError} if the data length does not match the dimensions,
 * or a rectangle field is not a non-negative integer.
 */
export const crop: Api["crop"] = api.crop;
/**
 * The largest `[width, height]` that fits inside `maxWidth` x `maxHeight`
 * with the same aspect ratio. Never enlarges.
 *
 * @throws {RangeError} if an argument is not an integer of at least 1.
 */
export const fitWithin: Api["fitWithin"] = api.fitWithin;
