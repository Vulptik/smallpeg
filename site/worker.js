// Runs the npm package's web build off the main thread.
import init, { encode, optimize, stripMetadata, fitWithin, resize } from "./pkg/smallpeg_wasm.js";

init().then(
  () => postMessage({ ready: true }),
  (e) => postMessage({ ready: false, error: String((e && e.message) || e) }),
);

function compress({ input, width, height, quality, fullChroma, maxSide }) {
  // Canvas pixels are RGBA; flatten alpha onto white, then reduce if asked.
  const rgba = new Uint8Array(input);
  let rgb = new Uint8Array(width * height * 3);
  for (let i = 0, j = 0; i < rgba.length; i += 4, j += 3) {
    const a = rgba[i + 3];
    for (let c = 0; c < 3; c++) rgb[j + c] = Math.round((rgba[i + c] * a + 255 * (255 - a)) / 255);
  }
  let [w, h] = [width, height];
  if (maxSide > 0) {
    const [nw, nh] = fitWithin(w, h, maxSide, maxSide);
    if (nw !== w || nh !== h) {
      rgb = resize(rgb, w, h, 3, nw, nh, true);
      [w, h] = [nw, nh];
    }
  }
  return encode(rgb, w, h, 3, quality, fullChroma);
}

onmessage = ({ data }) => {
  const t0 = performance.now();
  try {
    let out;
    if (data.lossless) {
      const opt = optimize(new Uint8Array(data.input));
      out = opt && (stripMetadata(opt) ?? opt);
      if (!out) throw new Error("This JPEG uses a coding that can't be re-coded losslessly. Turn off Lossless to re-encode it.");
    } else {
      out = compress(data);
    }
    const buf = out.buffer.slice(out.byteOffset, out.byteOffset + out.byteLength);
    postMessage({ id: data.id, ok: true, output: buf, ms: performance.now() - t0 }, [buf]);
  } catch (e) {
    postMessage({ id: data.id, ok: false, error: String((e && e.message) || e) });
  }
};
