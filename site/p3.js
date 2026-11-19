// Display P3: an ICC profile for it, built here from the standard's
// numbers, and the APP2 segments that attach a profile to a JPEG.

// The sRGB transfer function (to linear), which Display P3 shares.
const lin = (v) => (v <= 0.04045 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4);

const inv = (m) => {
  const [[a, b, c], [d, e, f], [g, h, i]] = m;
  const A = e * i - f * h, B = f * g - d * i, C = d * h - e * g;
  const det = a * A + b * B + c * C;
  return [
    [A / det, (c * h - b * i) / det, (b * f - c * e) / det],
    [B / det, (a * i - c * g) / det, (c * d - a * f) / det],
    [C / det, (b * g - a * h) / det, (a * e - b * d) / det],
  ];
};
const mul = (m, v) => m.map((r) => r[0] * v[0] + r[1] * v[1] + r[2] * v[2]);
const mm = (a, b) => a.map((r) => [0, 1, 2].map((j) => r[0] * b[0][j] + r[1] * b[1][j] + r[2] * b[2][j]));
const xyz = ([x, y]) => [x / y, 1, (1 - x - y) / y];
const D65 = xyz([0.3127, 0.329]);

// Linear RGB -> XYZ for the given primaries, with D65 white.
function toXyz(primaries) {
  const P = [0, 1, 2].map((r) => primaries.map((p) => xyz(p)[r])); // rows X, Y, Z
  const S = mul(inv(P), D65);
  return P.map((r) => r.map((v, j) => v * S[j]));
}
const P3 = toXyz([[0.68, 0.32], [0.265, 0.69], [0.15, 0.06]]); // SMPTE EG 432-1
const SRGB = toXyz([[0.64, 0.33], [0.3, 0.6], [0.15, 0.06]]);

// ICC v4 wants colorants adapted to its D50 white: the Bradford transform.
function colorants() {
  const BFD = [
    [0.8951, 0.2664, -0.1614],
    [-0.7502, 1.7135, 0.0367],
    [0.0389, -0.0685, 1.0296],
  ];
  const D50 = [0.9642, 1, 0.8249];
  const s = mul(BFD, D65), d = mul(BFD, D50);
  const chad = mm(inv(BFD), BFD.map((r, i) => r.map((v) => (v * d[i]) / s[i])));
  return { M50: mm(chad, P3), chad, D50 };
}

/** An ICC v4 display profile for Display P3 (about 600 bytes). */
export function displayP3Profile() {
  const { M50, chad, D50 } = colorants();
  const s15 = (v) => Math.round(v * 65536);
  const parts = [];
  const tag = (sig, bytes) => parts.push({ sig, bytes });
  const xyzTag = (x, y, z) => {
    const b = new DataView(new ArrayBuffer(20));
    b.setUint32(0, 0x58595a20); // 'XYZ '
    [x, y, z].forEach((v, i) => b.setInt32(8 + 4 * i, s15(v)));
    return new Uint8Array(b.buffer);
  };
  const mluc = (text) => {
    const b = new DataView(new ArrayBuffer(28 + 2 * text.length));
    b.setUint32(0, 0x6d6c7563); // 'mluc'
    b.setUint32(8, 1);
    b.setUint32(12, 12);
    b.setUint16(16, 0x656e); // 'en'
    b.setUint16(18, 0x5553); // 'US'
    b.setUint32(20, 2 * text.length);
    b.setUint32(24, 28);
    for (let i = 0; i < text.length; i++) b.setUint16(28 + 2 * i, text.charCodeAt(i));
    return new Uint8Array(b.buffer);
  };
  const sf32 = (m) => {
    const b = new DataView(new ArrayBuffer(8 + 36));
    b.setUint32(0, 0x73663332); // 'sf32'
    m.flat().forEach((v, i) => b.setInt32(8 + 4 * i, s15(v)));
    return new Uint8Array(b.buffer);
  };
  // The sRGB curve as parametric type 3: Y = (aX + b)^g if X >= d, else cX.
  const para = (() => {
    const b = new DataView(new ArrayBuffer(32));
    b.setUint32(0, 0x70617261); // 'para'
    b.setUint16(8, 3);
    [2.4, 1 / 1.055, 0.055 / 1.055, 1 / 12.92, 0.04045].forEach((v, i) => b.setInt32(12 + 4 * i, s15(v)));
    return new Uint8Array(b.buffer);
  })();
  tag("desc", mluc("Display P3"));
  tag("cprt", mluc("No copyright, use freely"));
  tag("wtpt", xyzTag(...D50));
  tag("chad", sf32(chad));
  ["rXYZ", "gXYZ", "bXYZ"].forEach((sig, j) => tag(sig, xyzTag(M50[0][j], M50[1][j], M50[2][j])));
  tag("rTRC", para);
  tag("gTRC", para);
  tag("bTRC", para);

  const tableEnd = 128 + 4 + 12 * parts.length;
  let size = tableEnd;
  const placed = new Map(); // shared data (the curve) is stored once
  const entries = parts.map(({ sig, bytes }) => {
    if (!placed.has(bytes)) {
      placed.set(bytes, size);
      size += (bytes.length + 3) & ~3;
    }
    return { sig, offset: placed.get(bytes), length: bytes.length };
  });
  const out = new Uint8Array(size);
  const v = new DataView(out.buffer);
  const sig = (at, s) => [...s].forEach((c, i) => (out[at + i] = c.charCodeAt(0)));
  v.setUint32(0, size);
  v.setUint32(8, 0x04300000); // version 4.3
  sig(12, "mntr");
  sig(16, "RGB ");
  sig(20, "XYZ ");
  v.setUint16(24, 2026); // creation date: 2026-01-01
  v.setUint16(26, 1);
  v.setUint16(28, 1);
  sig(36, "acsp");
  D50.forEach((c, i) => v.setInt32(68 + 4 * i, s15(c)));
  v.setUint32(128, parts.length);
  entries.forEach((e, i) => {
    sig(132 + 12 * i, e.sig);
    v.setUint32(136 + 12 * i, e.offset);
    v.setUint32(140 + 12 * i, e.length);
  });
  for (const [bytes, at] of placed) out.set(bytes, at);
  return out;
}

/**
 * Whether Display P3 pixels (RGBA) hold a color sRGB cannot show: one
 * that converts to sRGB out of range by more than the rounding of 8-bit
 * P3 values can explain. An sRGB image drawn into a P3 canvas never
 * counts. Pixels that are not opaque are skipped: the canvas stores them
 * premultiplied, so faint ones read back with exaggerated colors.
 */
export function outsideSrgb(rgba) {
  const L = new Float64Array(256).map((_, i) => lin(i / 255));
  // Half a code value either side, in linear light, at each value.
  const H = L.map((_, i) => (L[Math.min(i + 1, 255)] - L[Math.max(i - 1, 0)]) / 2);
  const m = mm(inv(SRGB), P3); // P3 linear -> sRGB linear
  for (let i = 0; i < rgba.length; i += 4) {
    if (rgba[i + 3] !== 255) continue;
    const c = [rgba[i], rgba[i + 1], rgba[i + 2]];
    for (const row of m) {
      let x = 0, tol = 0;
      for (let j = 0; j < 3; j++) {
        x += row[j] * L[c[j]];
        tol += Math.abs(row[j]) * H[c[j]];
      }
      if (x < -tol || x > 1 + tol) return true;
    }
  }
  return false;
}

/** `jpeg` with `icc` attached as APP2 ICC_PROFILE segments after any JFIF header. */
export function withIcc(jpeg, icc) {
  const head = new TextEncoder().encode("ICC_PROFILE\0");
  const max = 65533 - head.length - 2;
  const n = Math.ceil(icc.length / max);
  const segs = [];
  for (let k = 0; k < n; k++) {
    const part = icc.subarray(k * max, (k + 1) * max);
    const len = 2 + head.length + 2 + part.length;
    segs.push(Uint8Array.of(0xff, 0xe2, len >> 8, len & 255), head, Uint8Array.of(k + 1, n), part);
  }
  let at = 2; // after SOI, and after an APP0 JFIF segment, which must come first
  if (jpeg[2] === 0xff && jpeg[3] === 0xe0) at = 4 + ((jpeg[4] << 8) | jpeg[5]);
  const total = jpeg.length + segs.reduce((s, x) => s + x.length, 0);
  const out = new Uint8Array(total);
  out.set(jpeg.subarray(0, at));
  let p = at;
  for (const s of segs) {
    out.set(s, p);
    p += s.length;
  }
  out.set(jpeg.subarray(at), p);
  return out;
}
