// One way of loading the wasm per process (see the list in test.mjs):
// `node test-init.mjs <entry>:<case>`. Prints "<entry>:<case> ok" on success.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";

const which = process.argv[2];
const bytes = readFileSync(new URL("./web/smallpeg_wasm_bg.wasm", import.meta.url));
const response = () => new Response(bytes, { headers: { "content-type": "application/wasm" } });
const pixel = { data: new Uint8Array([200, 100, 50]), width: 1, height: 1 };

// Proves the wasm is live: a real encode comes back as JPEG bytes.
function works(lib) {
  const jpeg = lib.encode(pixel);
  assert.deepEqual([jpeg[0], jpeg[1]], [0xff, 0xd8]);
}

const [entry, form] = which.split(":");
const lib = await import(entry === "web" ? "smallpeg/web" : "smallpeg");

switch (form) {
  case "before-init":
    assert.throws(() => lib.encode(pixel), /init\(\)/);
    assert.throws(() => lib.decode(new Uint8Array(8)), /init\(\)/);
    break;
  case "init-bytes":
    await lib.init(bytes);
    break;
  case "init-module":
    await lib.init(new WebAssembly.Module(bytes));
    break;
  case "init-response":
    await lib.init(response());
    break;
  case "init-promise":
    await lib.init(Promise.resolve(response()));
    break;
  case "init-legacy-object":
    await lib.init({ module_or_path: bytes });
    break;
  case "init-default-export":
    await lib.default(bytes);
    break;
  case "initSync-bytes":
    lib.initSync(bytes);
    break;
  case "initSync-module":
    lib.initSync(new WebAssembly.Module(bytes));
    break;
  case "initSync-legacy-object":
    lib.initSync({ module: bytes });
    break;
  case "init-retry":
    // A failed load can be retried.
    await assert.rejects(lib.init(new Uint8Array([0, 1, 2, 3])));
    await lib.init(bytes);
    break;
  case "init-after-initSync":
    lib.initSync(bytes);
    await lib.init(); // no fetch: already loaded
    break;
  case "initSync-after-init":
    await lib.init(bytes);
    lib.initSync(new Uint8Array(0)); // already loaded: ignored
    break;
  case "initSync-during-init": {
    const pending = lib.init(bytes);
    assert.throws(() => lib.initSync(bytes), /still loading/);
    await pending;
    break;
  }
  case "init-wrong-object":
    // Our own error, not WebAssembly's on the wrapped object.
    await assert.rejects(lib.init({}), { name: "TypeError", message: /^smallpeg: init\(\) takes/ });
    await assert.rejects(lib.init({ module: bytes }), { name: "TypeError", message: /^smallpeg: init\(\) takes/ });
    await lib.init(bytes);
    break;
  case "require":
    // require(esm), which engines ">=22.12" is there for.
    {
      const { createRequire } = await import("node:module");
      const cjs = createRequire(import.meta.url)(entry === "web" ? "smallpeg/web" : "smallpeg");
      assert.equal(typeof cjs.encode, "function");
      assert.equal(cjs.default, cjs.init);
      if (entry === "web") cjs.initSync(bytes);
      works(cjs);
    }
    break;
  case "browser-style-calls":
    // Code written for the browser build, run under Node: all no-ops.
    await lib.init("https://example.com/smallpeg_wasm_bg.wasm");
    await lib.default({ module_or_path: bytes });
    lib.initSync(bytes);
    break;
  default:
    throw new Error(`unknown case ${which}`);
}
if (form !== "before-init") works(lib);
console.log(`${which} ok`);
