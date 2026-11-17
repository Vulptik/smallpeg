# How the encoder works

What `Encoder` does to make files small, and how its settings were
chosen.

## Trellis quantization

The simplest way to quantize is to round each DCT coefficient to the
nearest step. smallpeg instead chooses each block's coefficients by
weighing the error of each candidate value against its actual cost in
bits, using a first pass's symbol statistics as the price list.
Coefficients whose bits buy little quality are dropped. With the `Photo`
preset on RGB input, errors are also weighted by step size: an error of
one step costs more pixel error where the step is coarse. The DC
coefficients, which are coded as differences
between neighboring blocks, get the same treatment along each component's
chain.

## Huffman tables and scans

Every file gets its own optimal Huffman tables. For progressive output the
encoder measures the exact coded size of several scan layouts per
component (where to split the frequency bands, how many bits to send in
the first pass) and keeps the smallest. It then also computes the size of
baseline output and keeps whichever of the two is smaller; very smooth
images often come out smaller as baseline.

## Quantization presets

- `QuantPreset::Photo` (default): one fine-grained table for luma and
  chroma, which does better on photographs, smooth gradients and low
  quality settings.
- `QuantPreset::AnnexK`: the example tables from the JPEG standard, which
  do better on documents, scans and noisy content at high quality.

With 4:4:4 sampling and the `Photo` preset, chroma steps are coarsened
automatically, since full-resolution chroma otherwise spends bits the eye
does not see.

## Deringing

JPEG compression makes the pixels around sharp edges overshoot (ringing),
which shows as halos around text and line art. Where luma is at full
white, smallpeg raises those pixels past 255 along the neighboring edge's
slope before the transform. The block then needs fewer bits, and the extra
brightness is clamped away by every decoder. In the project's
measurements on rendered documents (Latin and Japanese text, tables,
charts), this made files 17 to 19% smaller at the same SSIMULACRA2 score;
photographs came out the same size (within 0.2%). It is on by default and
can be turned off with `Encoder::deringing(false)`, which saves encoding
time when re-compressing images that were already JPEGs.

## How settings were chosen

Encoder settings were chosen by measuring file size at equal quality on
documents, scans, gradients and photographs, with three metrics: PSNR,
SSIMULACRA2 and butteraugli. A change was kept only when it reduced size at
equal quality without making individual images notably worse.

## Speed and memory

Encoding is single-threaded unless the `parallel` feature is on. On one
core of an Apple M6 (native build), an 8-megapixel phone photo
(3264 x 2448) encodes in about 0.2 s at quality 80. WebAssembly is
somewhat slower than native.

Sources of the techniques and tables are listed in
[THIRD_PARTY.md](../THIRD_PARTY.md).
