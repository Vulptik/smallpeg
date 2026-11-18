// Node entry point: the wasm is loaded synchronously when this module is
// imported, so there is nothing to initialize. `init`, `initSync` and the
// default export mirror the browser build so code shared between the two
// runs unchanged; their arguments are ignored.

import * as glue from "../node/smallpeg_wasm.js";
import { type Api, makeApi, type ParamsMatch } from "./core.mjs";

// Fails to compile if the glue's parameters drift from what core expects.
type Assert<T extends { [K in keyof T]: true }> = T;
type _Check = Assert<ParamsMatch<typeof glue>>;

const api = makeApi(glue);

/** Does nothing in Node; present so code shared with the browser build can always `await init()`. */
export async function init(_wasm?: unknown): Promise<void> {}

/** Does nothing in Node; present so code shared with the browser build can call `initSync(bytes)`. */
export function initSync(_module?: unknown): void {}

export default init;

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
 * Turn a JPEG upright without decoding it to pixels: the EXIF orientation is applied
 * to the image and the tag set to 1, with no loss of quality. Mirrored
 * edges that are not a whole MCU (usually 8 or 16 pixels) are cut off,
 * up to 15 pixels in most files. `undefined` where `optimize` would return
 * it, and when the image is smaller than one MCU on a mirrored axis.
 */
export const upright: Api["upright"] = api.upright;
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
