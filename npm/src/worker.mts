// Worker entry point (browsers and bundlers): the same functions as the
// browser build, run in a Web Worker so the page stays responsive.

import { connect, type Port, type WorkerApi } from "./protocol.mjs";

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
export type { WorkerApi } from "./protocol.mjs";

/** Options for {@link createWorker}. */
export interface WorkerOptions {
  /**
   * Where the worker loads the `.wasm` from: a URL (string or `URL`,
   * relative ones resolved against the page), the bytes (`ArrayBuffer` or
   * `Uint8Array`, copied), or a compiled `WebAssembly.Module`. Unlike
   * `init`, a `Response` or a promise cannot be sent to a worker. By
   * default, the file inside the package. Typed loosely so the
   * declarations need no DOM types.
   */
  wasm?: string | object | undefined;
}

// What can be sent to the worker for init(), with URLs made absolute (a
// relative one would resolve against the worker's script, and a URL
// object cannot be cloned). undefined when `wasm` is none of those.
function wasmSource(wasm: unknown): { value: unknown } | undefined {
  if (wasm === undefined) return { value: undefined };
  if (typeof wasm === "string" || (typeof URL !== "undefined" && wasm instanceof URL)) {
    // As the page resolves it (a <base> element included); in a worker,
    // against the worker's own location.
    const g = globalThis as { document?: { baseURI: string }; location?: { href: string } };
    const base = g.document?.baseURI ?? g.location?.href;
    try {
      return { value: new URL(wasm, base).href };
    } catch {
      return undefined; // not a URL
    }
  }
  if (
    wasm instanceof ArrayBuffer ||
    wasm instanceof Uint8Array ||
    (typeof WebAssembly !== "undefined" && wasm instanceof WebAssembly.Module)
  ) {
    return { value: wasm };
  }
  return undefined;
}

/**
 * Start a Web Worker that runs the functions of the browser build, and
 * return them as functions that take the same arguments and return
 * promises. The worker loads the wasm by itself; there is no `init` to
 * call.
 *
 * Arguments are copied to the worker (your buffers stay usable); returned
 * bytes and image data are moved back without a copy. Calls on one worker
 * run one at a time, in order: create several workers to run in parallel.
 * Errors reject the promise with the same kind of error the function
 * throws.
 *
 * @throws {TypeError} where Web Workers are not available (such as Node).
 */
export function createWorker(options?: WorkerOptions): WorkerApi {
  if (typeof Worker === "undefined") throw new TypeError("smallpeg: createWorker needs Web Workers");
  if (options !== undefined && (typeof options !== "object" || options === null || Array.isArray(options))) {
    return failed(new TypeError("smallpeg: options must be an object"));
  }
  let source: { value: unknown } | undefined;
  try {
    source = wasmSource(options?.wasm);
  } catch (e) {
    // A getter or proxy that throws: a rejection like any other failure.
    return failed(e instanceof Error ? e : new TypeError(String(e)));
  }
  if (!source) {
    return failed(
      new TypeError("smallpeg: wasm must be a URL, an ArrayBuffer, a Uint8Array or a WebAssembly.Module"),
    );
  }
  let worker: Worker;
  try {
    // Written out in full so bundlers see the worker and bundle it.
    worker = new Worker(new URL("./worker-main.mjs", import.meta.url), { type: "module" });
  } catch (e) {
    // A worker script must come from the page's origin. Loaded from
    // another one (a CDN), start a same-origin worker that imports it;
    // module imports may cross origins (with CORS).
    try {
      // Not written as a literal: bundlers would copy the file unbundled.
      const name = "./worker-main.mjs";
      const script = new URL(name, import.meta.url).href;
      const blob = new Blob([`import ${JSON.stringify(script)};`], { type: "text/javascript" });
      const url = URL.createObjectURL(blob);
      try {
        worker = new Worker(url, { type: "module" });
      } finally {
        // The worker has its own reference to the script once created.
        setTimeout(() => URL.revokeObjectURL(url), 60_000);
      }
    } catch {
      return failed(
        new Error("smallpeg: the worker failed to start" + (e instanceof Error ? `: ${e.message}` : "")),
      );
    }
  }
  const c = connect(worker as unknown as Port, source.value, () => worker.terminate());
  worker.onerror = (e: ErrorEvent) => {
    e.preventDefault();
    c.fail(new Error("smallpeg: the worker failed" + (e.message ? `: ${e.message}` : " to start")));
  };
  return c.api;
}

// A client with no worker, whose `ready` and every call reject with `e`.
function failed(e: Error): WorkerApi {
  const none: Port = { postMessage() {}, onmessage: null, onmessageerror: null };
  const c = connect(none, undefined, () => {});
  c.fail(e);
  return c.api;
}
