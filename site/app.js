import { outsideSrgb } from "./p3.js";

const $ = (id) => document.getElementById(id);
const stage = $("stage");

const fmt = (n) => {
  if (n >= 1048576) return (n / 1048576).toFixed(n >= 104857600 ? 0 : 1) + " MB";
  if (n >= 1024) return Math.round(n / 1024) + " KB";
  return n + " B";
};
const setState = (s) => (stage.dataset.state = s);

// ---- worker lifecycle ----------------------------------------------------
// The engine is the npm package's web build, run in a module worker. One
// worker, one job at a time; cancelling or changing a setting mid-job
// terminates it and starts a fresh one, because wasm cannot be interrupted.
let worker = null;
let engineReady = false;
let jobId = 0;
let running = null; // { id, name, size, timer }
let pendingRun = false;

function spawn() {
  engineReady = false;
  worker = new Worker(new URL("./worker.js", import.meta.url), { type: "module" });
  worker.onmessage = ({ data }) => {
    if ("ready" in data) {
      engineReady = data.ready;
      $("engine").textContent = data.ready
        ? "Processed on this device — nothing is uploaded."
        : "Couldn't load the engine: " + data.error;
      if (data.ready && pendingRun) {
        pendingRun = false;
        run();
      }
      return;
    }
    if (!running || data.id !== running.id) return;
    finish(data);
  };
  worker.onerror = (e) => {
    if (running) {
      stopTimer();
      showError(running.name, "The engine stopped unexpectedly" + (e.message ? `: ${e.message}` : "") + ". The image may be too large for this browser.");
      running = null;
    }
    restart();
  };
}
function restart() {
  worker?.terminate();
  spawn();
}
spawn();

// ---- input --------------------------------------------------------------
// file: { name, bytes, isJpeg, url, rgba, width, height }
let file = null;

async function takeFile(f) {
  if (!f) return;
  markSample(null);
  if (!/^image\/(jpeg|png|webp)$/.test(f.type) && !/\.(jpe?g|png|webp)$/i.test(f.name)) {
    showError(f.name, "That doesn't look like a JPEG, PNG or WebP image.");
    return;
  }
  $("workName").textContent = f.name;
  setState("working");
  try {
    const bytes = await f.arrayBuffer();
    const head = new Uint8Array(bytes, 0, Math.min(2, bytes.byteLength));
    // Decode with the browser (applies EXIF orientation) to get pixels.
    const bmp = await createImageBitmap(f);
    const { rgba, p3 } = pixels(bmp);
    const width = bmp.width, height = bmp.height;
    bmp.close?.();
    if (file) URL.revokeObjectURL(file.url);
    file = { name: f.name, bytes, isJpeg: head[0] === 0xff && head[1] === 0xd8, url: URL.createObjectURL(f), rgba, p3, width, height };
  } catch {
    showError(f.name, "This browser couldn't read that image.");
    return;
  }
  $("lossless").disabled = !file.isJpeg;
  if (!file.isJpeg) $("lossless").checked = false;
  syncSettings();
  run();
}

const drop = $("drop");
$("file").addEventListener("change", (e) => {
  takeFile(e.target.files[0]);
  e.target.value = "";
});
// Accept drops anywhere on the page, highlighting the drop zone.
let dragDepth = 0;
addEventListener("dragenter", (e) => {
  if (![...e.dataTransfer.types].includes("Files")) return;
  dragDepth++;
  drop.classList.add("over");
  if (stage.dataset.state !== "working") setState("idle");
});
addEventListener("dragleave", () => {
  if (--dragDepth <= 0) {
    dragDepth = 0;
    drop.classList.remove("over");
  }
});
addEventListener("dragover", (e) => e.preventDefault());
addEventListener("drop", (e) => {
  e.preventDefault();
  dragDepth = 0;
  drop.classList.remove("over");
  takeFile(e.dataTransfer.files[0]);
});

// The image's pixels as RGBA. A canvas is sRGB unless asked otherwise, and
// drawing a wide-gamut photo (Display P3, as from an iPhone) into one
// dulls its most saturated colors. So read it in Display P3 where the
// browser can, and keep that when some color is outside sRGB (the worker
// then attaches a Display P3 profile); otherwise read it in sRGB as before.
function pixels(bmp) {
  const read = (colorSpace) => {
    const cv = document.createElement("canvas");
    cv.width = bmp.width;
    cv.height = bmp.height;
    const cx = cv.getContext("2d", { colorSpace, willReadFrequently: true });
    if (!cx || (cx.getContextAttributes?.().colorSpace ?? "srgb") !== colorSpace) return null;
    cx.drawImage(bmp, 0, 0);
    const data = cx.getImageData(0, 0, bmp.width, bmp.height, { colorSpace }).data;
    cv.width = cv.height = 0; // free the canvas now, not at garbage collection
    return data;
  };
  let wide = null;
  try {
    wide = read("display-p3");
  } catch {}
  if (wide && outsideSrgb(wide)) return { rgba: wide.buffer, p3: true };
  wide = null;
  return { rgba: read("srgb").buffer, p3: false };
}

// ---- settings -----------------------------------------------------------
// The settings never submit anywhere (Enter in a field would reload the page).
$("settings").addEventListener("submit", (e) => e.preventDefault());
const settings = () => ({
  quality: Number($("quality").value),
  maxSide: Number($("maxSide").value),
  fullChroma: $("fullChroma").checked,
  lossless: $("lossless").checked && !$("lossless").disabled,
});
function syncSettings() {
  $("qOut").textContent = $("quality").value;
  const off = settings().lossless;
  for (const id of ["quality", "maxSide", "fullChroma"]) $(id).disabled = off;
  $("settings").classList.toggle("off", off);
}
let rerunTimer = 0;
function settingsChanged() {
  syncSettings();
  if (!file) return;
  clearTimeout(rerunTimer);
  rerunTimer = setTimeout(run, 350);
}
$("quality").addEventListener("input", settingsChanged);
for (const id of ["maxSide", "fullChroma", "lossless"]) $(id).addEventListener("change", settingsChanged);

// ---- running ------------------------------------------------------------
function run() {
  if (!file) return;
  if (running) {
    stopTimer();
    running = null;
    restart();
  }
  $("workName").textContent = file.name;
  setState("working");
  if (!engineReady) {
    pendingRun = true;
    return;
  }
  const id = ++jobId;
  const t0 = performance.now();
  const s = settings();
  running = {
    id, name: file.name, size: file.bytes.byteLength, s,
    timer: setInterval(() => ($("elapsed").textContent = ((performance.now() - t0) / 1000).toFixed(1)), 100),
  };
  $("elapsed").textContent = "0.0";
  // Copy: the original stays here so settings can be changed and re-run.
  const input = (s.lossless ? file.bytes : file.rgba).slice(0);
  worker.postMessage({ id, input, width: file.width, height: file.height, p3: file.p3, ...s }, [input]);
}
function stopTimer() {
  if (running) clearInterval(running.timer);
}

function finish(res) {
  const job = running;
  stopTimer();
  running = null;
  if (!res.ok) {
    showError(job.name, res.error || "Something went wrong.");
    restart();
    return;
  }
  const before = job.size;
  let out = res.output;
  let after = out.byteLength;
  // Never hand back something bigger, unless the size was reduced on
  // request. Lossless output is never swapped for the original: it is the
  // copy with the metadata removed, which the original still has.
  const kept = after >= before && !job.s.lossless && job.s.maxSide === 0 && file.isJpeg;
  const lean = kept || (job.s.lossless && after >= before);
  if (kept) {
    out = file.bytes;
    after = before;
  }
  const saved = 1 - after / before;
  const grew = after > before;
  $("doneName").textContent = job.name;
  $("pct").textContent = lean ? "0%" : (grew ? "+" : "−") + (Math.abs(saved) * 100).toFixed(Math.abs(saved) >= 0.1 ? 0 : 1) + "%";
  $("pctLabel").textContent = kept
    ? "already lean — original kept"
    : lean
      ? "already lean — nothing to remove"
      : grew
        ? "larger than the original"
        : "smaller";
  $("sizeBefore").textContent = fmt(before);
  $("sizeAfter").textContent = fmt(after);
  $("barBefore").style.width = "0";
  $("barAfter").style.width = "0";
  const top = Math.max(before, after);
  requestAnimationFrame(() => requestAnimationFrame(() => {
    $("barBefore").style.width = (before / top) * 100 + "%";
    $("barAfter").style.width = Math.max(1, (after / top) * 100) + "%";
  }));

  const a = $("download");
  if (a.href) URL.revokeObjectURL(a.href);
  const url = URL.createObjectURL(new Blob([out], { type: "image/jpeg" }));
  a.href = url;
  a.download = job.name.replace(/\.[a-z0-9]+$/i, "") + (lean ? ".jpg" : ".small.jpg");

  const outImg = $("outImg");
  outImg.onload = () => {
    const how = job.s.lossless ? "lossless" : `quality ${job.s.quality}, ${job.s.fullChroma ? "4:4:4" : "4:2:0"}`;
    const dims = outImg.naturalWidth === file.width ? `${file.width}×${file.height}` : `${file.width}×${file.height} → ${outImg.naturalWidth}×${outImg.naturalHeight}`;
    $("meta").textContent = `${dims} · ${how} · ${(res.ms / 1000).toFixed(2)} s`;
  };
  $("meta").textContent = "";
  outImg.src = url;
  $("origImg").src = file.url;
  setState("done");
}

function showError(name, msg) {
  $("errName").textContent = name;
  $("errMsg").textContent = msg;
  setState("error");
}

$("cancel").addEventListener("click", () => {
  stopTimer();
  running = null;
  pendingRun = false;
  file = null;
  restart();
  setState("idle");
});
// Back to the drop zone and the samples.
for (const id of ["again", "errAgain"]) {
  $(id).addEventListener("click", () => {
    file = null;
    markSample(null);
    setState("idle");
  });
}

// ---- sample photos -------------------------------------------------------
const SAMPLES = [
  ["autumn-leaves", "Autumn leaves"],
  ["green-canopy", "Green canopy"],
  ["beach", "Beach"],
  ["lagoon", "Lagoon"],
].map(([slug, title]) => ({ slug, title }));
function markSample(i) {
  document.querySelectorAll(".sample").forEach((b, j) => b.setAttribute("aria-pressed", String(j === i)));
}
SAMPLES.forEach((s, i) => {
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "sample";
  btn.setAttribute("aria-pressed", "false");
  btn.title = s.title;
  const img = document.createElement("img");
  img.src = `samples/${s.slug}.thumb.jpg`;
  img.width = 64;
  img.height = 48;
  img.alt = s.title;
  btn.append(img);
  btn.addEventListener("click", async () => {
    let blob;
    try {
      const r = await fetch(`samples/${s.slug}.jpg`);
      if (!r.ok) throw new Error(r.status);
      blob = await r.blob();
    } catch {
      showError(s.title, "Couldn't load the sample photo.");
      return;
    }
    await takeFile(new File([blob], `${s.slug}.jpg`, { type: "image/jpeg" }));
    markSample(i);
  });
  $("samples").append(btn);
});

// ---- side-by-side comparison ----------------------------------------------
const cmp = $("cmp"), view = $("cmpView"), cO = $("cmpOrig"), cR = $("cmpOut");
let cs = { w: 0, h: 0, scale: 1, x: 0, y: 0, split: 0.5, fit: true, opener: null };
function cmpLayout() {
  const vw = view.clientWidth, vh = view.clientHeight;
  if (cs.fit) {
    cs.scale = Math.min(vw / cs.w, vh / cs.h);
    cs.x = (vw - cs.w * cs.scale) / 2;
    cs.y = (vh - cs.h * cs.scale) / 2;
  }
  // Whole pixels only: a fractional offset makes the browser resample the
  // image on every move, which shows as shimmer when zoomed in.
  cs.x = Math.round(cs.x);
  cs.y = Math.round(cs.y);
  const sx = Math.round(cs.split * vw);
  // The result sits in a window that starts at the divider; moving the
  // divider moves the window, never the image.
  $("cmpClip").style.left = sx + "px";
  cO.style.transform = `translate(${cs.x}px, ${cs.y}px) scale(${cs.scale})`;
  cR.style.transform = `translate(${cs.x - sx}px, ${cs.y}px) scale(${cs.scale})`;
  for (const im of [cO, cR]) { im.style.width = cs.w + "px"; im.style.height = cs.h + "px"; }
  $("cmpDivider").style.left = sx + "px";
  view.classList.toggle("px", cs.scale > 1);
  cmp.querySelectorAll("[data-zoom]").forEach((b) =>
    b.setAttribute("aria-pressed", String(b.dataset.zoom === "fit" ? cs.fit : !cs.fit && Math.abs(cs.scale - Number(b.dataset.zoom)) < 1e-6)));
}
function cmpZoom(scale, cx, cy) {
  const vw = view.clientWidth, vh = view.clientHeight;
  cx ??= vw / 2; cy ??= vh / 2;
  const ix = (cx - cs.x) / cs.scale, iy = (cy - cs.y) / cs.scale;
  cs.fit = false; cs.scale = scale; cs.x = cx - ix * scale; cs.y = cy - iy * scale;
  cmpLayout();
}
function openCompare() {
  cs = { ...cs, w: $("outImg").naturalWidth, h: $("outImg").naturalHeight, split: 0.5, fit: true, opener: document.activeElement };
  cO.src = $("origImg").src;
  cR.src = $("outImg").src;
  $("cmpTitle").textContent = $("doneName").textContent;
  $("cmpOrigSize").textContent = $("sizeBefore").textContent;
  $("cmpOutSize").textContent = $("sizeAfter").textContent;
  cmp.hidden = false;
  document.body.style.overflow = "hidden";
  cmpLayout();
  $("cmpHandle").focus();
}
function closeCompare() {
  cmp.hidden = true;
  document.body.style.overflow = "";
  cs.opener?.focus();
}
$("expand").addEventListener("pointerdown", (e) => e.stopPropagation());
$("expand").addEventListener("click", (e) => { e.stopPropagation(); openCompare(); });
$("cmpClose").addEventListener("click", closeCompare);
cmp.querySelectorAll("[data-zoom]").forEach((b) => b.addEventListener("click", () => {
  if (b.dataset.zoom === "fit") { cs.fit = true; cmpLayout(); } else cmpZoom(Number(b.dataset.zoom));
}));
addEventListener("keydown", (e) => {
  if (cmp.hidden) return;
  if (e.key === "Escape") closeCompare();
  else if (document.activeElement === $("cmpHandle") && (e.key === "ArrowLeft" || e.key === "ArrowRight")) {
    e.preventDefault();
    cs.split = Math.min(1, Math.max(0, cs.split + (e.key === "ArrowRight" ? 0.02 : -0.02)));
    cmpLayout();
  }
});
addEventListener("resize", () => { if (!cmp.hidden) cmpLayout(); });
let drag = null;
view.addEventListener("pointerdown", (e) => {
  view.setPointerCapture(e.pointerId);
  const onHandle = e.target.closest(".cmp-divider");
  drag = { mode: onHandle ? "split" : "pan", x: e.clientX, y: e.clientY, ox: cs.x, oy: cs.y };
  view.classList.add(drag.mode === "pan" ? "panning" : "splitting");
});
view.addEventListener("pointermove", (e) => {
  if (!drag) return;
  if (drag.mode === "split") {
    const r = view.getBoundingClientRect();
    cs.split = Math.min(1, Math.max(0, (e.clientX - r.left) / r.width));
  } else {
    cs.fit = false; cs.x = drag.ox + e.clientX - drag.x; cs.y = drag.oy + e.clientY - drag.y;
  }
  cmpLayout();
});
for (const ev of ["pointerup", "pointercancel"]) view.addEventListener(ev, () => { drag = null; view.classList.remove("panning", "splitting"); });
view.addEventListener("wheel", (e) => {
  e.preventDefault();
  const r = view.getBoundingClientRect();
  cmpZoom(Math.min(8, Math.max(0.05, cs.scale * (e.deltaY < 0 ? 1.2 : 1 / 1.2))), e.clientX - r.left, e.clientY - r.top);
}, { passive: false });

// ---- hold to compare ----------------------------------------------------
const peek = $("peek");
function hold(on) {
  peek.classList.toggle("hold", on);
  $("peekLabel").textContent = on ? "Original" : "Result · hold to see the original";
}
peek.addEventListener("pointerdown", (e) => { peek.setPointerCapture(e.pointerId); hold(true); });
for (const ev of ["pointerup", "pointercancel"]) peek.addEventListener(ev, () => hold(false));
peek.addEventListener("keydown", (e) => { if (e.key === " " || e.key === "Enter") { e.preventDefault(); hold(true); } });
peek.addEventListener("keyup", () => hold(false));
peek.addEventListener("blur", () => hold(false));

// ---- syntax colors --------------------------------------------------------
// A small tokenizer for the Rust and JavaScript samples: enough to color
// comments, strings, numbers, keywords, function names and types. The
// text itself is unchanged, so copying still gets the plain code.
const KEYWORDS = new Set(
  ("as async await break const continue else enum fn for from if impl import in let " +
    "loop match mod move mut new pub return self static struct true false use where while " +
    "export function type typeof void null undefined").split(" "),
);
const TOKEN =
  /(\/\/[^\n]*|\/\*[\s\S]*?\*\/)|("(?:[^"\\\n]|\\.)*"|`(?:[^`\\]|\\.)*`|'(?:[^'\\\n]|\\.)*'(?!\w))|(\b(?:0x[\da-fA-F_]+|\d[\d_]*(?:\.\d+)?(?:[eE][+-]?\d+)?)\b)|([A-Za-z_]\w*)(?=\s*(?:!\s*)?[(\[])|([A-Za-z_]\w*)/g;
// Rust's primitive types read as types, like the capitalized ones.
const PRIMITIVES = new Set("u8 u16 u32 u64 usize i8 i16 i32 i64 isize f32 f64 bool char str".split(" "));
const esc = (t) => t.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
function highlight(code) {
  const text = code.textContent;
  let html = "";
  let last = 0;
  for (const m of text.matchAll(TOKEN)) {
    html += esc(text.slice(last, m.index));
    const [tok, comment, str, num, call, word] = m;
    const name = call || word;
    const cls = comment ? "c" : str ? "s" : num ? "n"
      : /^[A-Z]/.test(name) || PRIMITIVES.has(name) ? "t" // types, and Some(...) / Ok(...)
      : KEYWORDS.has(name) && !(call && name === "new") && !/^\s*:(?!:)/.test(text.slice(m.index + tok.length))
        ? "k" // not Encoder::new() (a call) or { type: ... } (a key)
      : call ? "f" : "";
    html += cls ? `<span class="${cls}">${esc(tok)}</span>` : esc(tok);
    last = m.index + tok.length;
  }
  code.innerHTML = html + esc(text.slice(last));
}
for (const code of document.querySelectorAll(".code-card pre code")) highlight(code);

// ---- code tabs ----------------------------------------------------------
// Each code card has its own tab set.
for (const card of document.querySelectorAll(".code-card")) {
  const tabs = [...card.querySelectorAll('[role="tab"]')];
  const select = (tab) => {
    for (const t of tabs) {
      const on = t === tab;
      t.setAttribute("aria-selected", on);
      t.tabIndex = on ? 0 : -1;
      $(t.getAttribute("aria-controls")).hidden = !on;
    }
  };
  tabs.forEach((t, i) => {
    t.addEventListener("click", () => select(t));
    t.addEventListener("keydown", (e) => {
      const d = e.key === "ArrowRight" ? 1 : e.key === "ArrowLeft" ? -1 : 0;
      if (!d) return;
      const next = tabs[(i + d + tabs.length) % tabs.length];
      select(next);
      next.focus();
    });
  });
  const copy = card.querySelector(".copy");
  copy.addEventListener("click", () => copyText(copy, card.querySelector('[role="tabpanel"]:not([hidden])').textContent));
}
for (const b of document.querySelectorAll(".copy-cmd")) {
  b.addEventListener("click", () => copyText(b, b.dataset.copy));
}
async function copyText(button, text) {
  try {
    await navigator.clipboard.writeText(text);
    button.textContent = "Copied";
  } catch {
    button.textContent = "Select & copy";
  }
  setTimeout(() => (button.textContent = "Copy"), 1500);
}

// ---- theme --------------------------------------------------------------
// Follows the OS until the button is used; the choice is remembered.
const root = document.documentElement;
const osDark = matchMedia("(prefers-color-scheme: dark)");
const isDark = () => (root.dataset.theme ? root.dataset.theme === "dark" : osDark.matches);
function syncThemeButton() {
  $("theme").setAttribute("aria-label", isDark() ? "Switch to light theme" : "Switch to dark theme");
}
$("theme").addEventListener("click", () => {
  const next = isDark() ? "light" : "dark";
  if ((next === "dark") === osDark.matches) delete root.dataset.theme;
  else root.dataset.theme = next;
  try {
    if (root.dataset.theme) localStorage.setItem("theme", root.dataset.theme);
    else localStorage.removeItem("theme");
  } catch {}
  syncThemeButton();
});
osDark.addEventListener("change", syncThemeButton);
syncThemeButton();
syncSettings();
