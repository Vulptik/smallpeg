// Runs the npm package's browser entry off the main thread.
import { init, encode, optimize, stripMetadata, fitWithin, resize } from "./pkg/dist/web.mjs";

init().then(
  () => postMessage({ ready: true }),
  (e) => postMessage({ ready: false, error: String((e && e.message) || e) }),
);

function compress({ input, width, height, quality, fullChroma, maxSide }) {
  // Canvas pixels are RGBA; flatten alpha onto white, then reduce if asked.
  const rgba = new Uint8Array(input);
  const rgb = new Uint8Array(width * height * 3);
  for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
    const a = rgba[i + 3];
    for (let c = 0; c < 3; c++) rgb[j + c] = Math.round((rgba[i + c] * a + 255 * (255 - a)) / 255);
  }
  let image = { data: rgb, width, height, channels: 3 };
  if (maxSide > 0) {
    const [w, h] = fitWithin(width, height, maxSide, maxSide);
    if (w !== width || h !== height) image = resize(image, w, h, { linear: true });
  }
  return encode(image, { quality, fullColor: fullChroma });
}

onmessage = ({ data }) => {
  const t0 = performance.now();
  try {
    let out;
    if (data.lossless) {
      const input = new Uint8Array(data.input);
      const opt = optimize(input);
      if (!opt) throw new Error("This JPEG uses a coding that can't be re-coded losslessly. Turn off Lossless to re-encode it.");
      // Lossless promises the metadata is gone, so never fall back to a
      // copy that still has it: when re-coding does not pay, use the
      // original with its metadata removed instead.
      const clean = stripMetadata(opt);
      const plain = stripMetadata(input);
      if (!clean || !plain) throw new Error("Couldn't remove the metadata from this JPEG.");
      out = plain.byteLength < clean.byteLength ? plain : clean;
    } else {
      out = compress(data);
    }
    const buf = out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength);
    postMessage({ id: data.id, ok: true, output: buf, ms: performance.now() - t0 }, [buf]);
  } catch (e) {
    postMessage({ id: data.id, ok: false, error: String((e && e.message) || e) });
  }
};
