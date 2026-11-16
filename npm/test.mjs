// Smoke test for both builds: run `./npm/build.sh` first, then `node npm/test.mjs`.
import assert from "node:assert/strict";
import { createRequire } from "node:module";
import { readFileSync } from "node:fs";

const require = createRequire(import.meta.url);
const node = require("./node/smallpeg_wasm.js");
const web = await import("./web/smallpeg_wasm.js");
web.initSync({ module: readFileSync(new URL("./web/smallpeg_wasm_bg.wasm", import.meta.url)) });

function gradient(w, h, channels) {
  const px = new Uint8Array(w * h * channels);
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) {
      const i = (y * w + x) * channels;
      px[i] = (x * 255) / w;
      if (channels >= 3) {
        px[i + 1] = (y * 255) / h;
        px[i + 2] = 128;
      }
      if (channels === 4) px[i + 3] = 255;
    }
  }
  return px;
}

function run(lib, name) {
  const [w, h] = [96, 64];

  for (const ch of [1, 3, 4]) {
    const jpeg = lib.encode(gradient(w, h, ch), w, h, ch, 80);
    assert.equal(jpeg[0], 0xff, `${name}: SOI for ${ch} channel(s)`);
    assert.equal(jpeg[1], 0xd8);
    const info = lib.info(jpeg);
    assert.deepEqual([info.width, info.height, info.components], [w, h, ch === 1 ? 1 : 3]);
    const img = lib.decode(jpeg);
    assert.equal(img.pixels.length, w * h * (ch === 1 ? 1 : 3));
  }

  // Defaults and full color.
  const rgb = gradient(w, h, 3);
  const a = lib.encode(rgb, w, h, 3);
  const b = lib.encode(rgb, w, h, 3, 80, true);
  assert.ok(a.length > 0 && b.length > 0);

  // Lossless paths keep pixels.
  const opt = lib.optimize(a);
  assert.deepEqual(lib.decode(opt).pixels, lib.decode(a).pixels, `${name}: optimize keeps pixels`);
  const base = lib.optimize(a, true);
  assert.equal(lib.info(base).progressive, false);
  const clean = lib.stripMetadata(a);
  assert.deepEqual(lib.decode(clean).pixels, lib.decode(a).pixels);

  // Untrusted input: undefined, not an exception.
  assert.equal(lib.decode(new Uint8Array([1, 2, 3])), undefined);
  assert.equal(lib.optimize(new Uint8Array([0xff, 0xd8, 0xff])), undefined);
  assert.equal(lib.info(new Uint8Array(0)), undefined);

  // Pixel helpers.
  assert.deepEqual([...lib.fitWithin(4000, 3000, 1600, 1600)], [1600, 1200]);
  assert.equal(lib.resize(rgb, w, h, 3, 48, 32).length, 48 * 32 * 3);
  assert.equal(lib.resize(rgb, w, h, 3, 48, 32, true).length, 48 * 32 * 3);
  assert.equal(lib.crop(rgb, w, h, 3, 10, 5, 20, 10).length, 20 * 10 * 3);
  assert.equal(lib.crop(rgb, w, h, 3, 90, 0, 20, 10), undefined);
  const turned = lib.applyOrientation(rgb, w, h, 3, 6);
  assert.deepEqual([turned.width, turned.height], [h, w]);

  // Argument mistakes are JavaScript errors, and the instance keeps working.
  assert.throws(() => lib.encode(rgb, w, h + 1, 3), /expected/);
  assert.throws(() => lib.encode(rgb, w, h, 2), /channels/);
  assert.throws(() => lib.resize(rgb, w, h, 3, 0, 10), /at least 1/);
  assert.ok(lib.encode(rgb, w, h, 3).length > 0, `${name}: usable after an error`);

  console.log(`${name}: ok`);
}

run(node, "node build");
run(web, "web build");
