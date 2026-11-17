// Type test, DOM part: compiled only by test-types.tsconfig.json (which
// has the DOM lib). A canvas ImageData encodes as is, and the output goes
// straight into a Blob or an ImageData, which needs the output typed
// Uint8Array<ArrayBuffer> (TypeScript 5.7 or later).

import { encode, type Image } from "smallpeg/web";

declare const imageData: ImageData;

const blob = new Blob([encode(imageData)], { type: "image/jpeg" });
// 4-channel results (RGBA) view straight into an ImageData.
declare const rgbaImage: Image;
if (rgbaImage.channels === 4) {
  new ImageData(new Uint8ClampedArray(rgbaImage.data.buffer), rgbaImage.width);
}
void blob;
