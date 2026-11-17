# Color profiles

The encoder takes raw RGB bytes and never sees the source file, so it
cannot know what color space those bytes are in. Output carries no ICC
profile unless you attach one, and decoders then read it as sRGB.

If the source is in another space, colors shift visibly unless you forward
the source's profile. Display P3 is the usual case: iPhone photos, macOS
screenshots and many renders.

```rust
let mut enc = smallpeg::Encoder::new(85);
if let Some(icc) = &source_icc_profile {
    // e.g. the profile your image decoder read from the source file
    enc = enc.icc_profile(icc);
}
let jpeg = enc.encode_rgb(&rgb, width, height);
```

Alternatively, convert the pixels to sRGB before encoding and omit the
profile.

Many image decoding crates hand back raw pixels and leave the profile
aside, so it is easy to drop by accident.

Two operations are not affected:

- `optimize_jpeg` and `strip_metadata` keep the input's ICC profile as it
  is.
- In the browser, pixels read from a canvas are already in the canvas's
  color space (sRGB by default), so encoding them without a profile is
  correct.
