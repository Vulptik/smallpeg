//! JavaScript bindings for smallpeg, built into the npm package.
//!
//! Every entry point validates its arguments and reports a mismatch as a
//! JavaScript `Error` instead of letting the library's assertions trap the
//! WebAssembly instance. Functions that take untrusted JPEG bytes return
//! `undefined` for input they cannot handle, like the Rust API's `None`.

use wasm_bindgen::prelude::*;

const DEFAULT_QUALITY: u8 = 80;

/// Pixels plus their layout, returned by `decode` and `applyOrientation`.
#[wasm_bindgen]
pub struct Image {
    width: u32,
    height: u32,
    channels: u8,
    pixels: Vec<u8>,
}

#[wasm_bindgen]
impl Image {
    /// Width in pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 1 (gray) or 3 (RGB) for decoded JPEGs; whatever was passed in for
    /// `applyOrientation`.
    #[wasm_bindgen(getter)]
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// `width * height * channels` bytes, interleaved, top row first.
    #[wasm_bindgen(getter)]
    pub fn pixels(&self) -> Vec<u8> {
        self.pixels.clone()
    }
}

/// Frame-header facts about a JPEG, returned by `info`.
#[wasm_bindgen]
pub struct JpegInfo {
    width: u32,
    height: u32,
    components: u8,
    progressive: bool,
    orientation: u8,
}

#[wasm_bindgen]
impl JpegInfo {
    /// Width in pixels.
    #[wasm_bindgen(getter)]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// Height in pixels.
    #[wasm_bindgen(getter)]
    pub fn height(&self) -> u32 {
        self.height
    }

    /// 1 (gray), 3 (color) or 4 (CMYK / YCCK).
    #[wasm_bindgen(getter)]
    pub fn components(&self) -> u8 {
        self.components
    }

    /// True for progressive files.
    #[wasm_bindgen(getter)]
    pub fn progressive(&self) -> bool {
        self.progressive
    }

    /// EXIF orientation, 1..=8 (1 when the file has none).
    #[wasm_bindgen(getter)]
    pub fn orientation(&self) -> u8 {
        self.orientation
    }
}

fn check_pixels(len: usize, width: u32, height: u32, channels: u32) -> Result<(), JsError> {
    if width == 0 || height == 0 {
        return Err(JsError::new("width and height must be at least 1"));
    }
    let want = width as u64 * height as u64 * channels as u64;
    if len as u64 != want {
        return Err(JsError::new(&format!(
            "expected {want} bytes for {width}x{height} with {channels} channel(s), got {len}"
        )));
    }
    Ok(())
}

/// Encode pixels as a JPEG.
///
/// `channels` is 1 (gray), 3 (RGB) or 4 (RGBA, as from a canvas; alpha is
/// flattened onto white). `quality` is 1..=100 (default 80). `fullColor`
/// keeps chroma at full resolution (4:4:4) instead of halving it (4:2:0):
/// sharper colored text and lines, larger files. Both dimensions must be at
/// most 65535.
#[allow(non_snake_case)] // parameter names are the JavaScript ones
#[wasm_bindgen]
pub fn encode(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    quality: Option<u8>,
    fullColor: Option<bool>,
) -> Result<Vec<u8>, JsError> {
    if !matches!(channels, 1 | 3 | 4) {
        return Err(JsError::new("channels must be 1, 3 or 4"));
    }
    check_pixels(pixels.len(), width, height, channels)?;
    if width > 65535 || height > 65535 {
        return Err(JsError::new("JPEG dimensions are limited to 65535"));
    }
    let sampling = if fullColor.unwrap_or(false) {
        smallpeg::Sampling::S444
    } else {
        smallpeg::Sampling::S420
    };
    let enc = smallpeg::Encoder::new(quality.unwrap_or(DEFAULT_QUALITY)).sampling(sampling);
    Ok(match channels {
        1 => enc.encode_gray(pixels, width, height),
        3 => enc.encode_rgb(pixels, width, height),
        _ => {
            let mut rgb = Vec::with_capacity(pixels.len() / 4 * 3);
            for p in pixels.as_chunks::<4>().0 {
                let a = p[3] as u32;
                for &c in &p[..3] {
                    rgb.push(((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8);
                }
            }
            enc.encode_rgb(&rgb, width, height)
        }
    })
}

/// Make an existing JPEG smaller without changing a pixel. `undefined` if
/// the file is not one this can re-code. The result can come out larger
/// for files that are already optimized; compare lengths. `baseline`
/// forces baseline sequential output (for targets that dislike
/// progressive JPEGs, such as PDF embedding).
#[wasm_bindgen]
pub fn optimize(jpeg: &[u8], baseline: Option<bool>) -> Option<Vec<u8>> {
    if baseline.unwrap_or(false) {
        smallpeg::optimize_jpeg_baseline(jpeg)
    } else {
        smallpeg::optimize_jpeg(jpeg)
    }
}

/// Remove EXIF (including GPS), XMP, comments and other metadata without
/// touching the image data. Keeps the color profile and the photo's
/// orientation. `undefined` if the file cannot be parsed.
#[wasm_bindgen(js_name = stripMetadata)]
pub fn strip_metadata(jpeg: &[u8]) -> Option<Vec<u8>> {
    smallpeg::strip_metadata(jpeg)
}

/// Read a JPEG's frame header without decoding it. `undefined` if the
/// bytes are not a JPEG this library understands.
#[wasm_bindgen]
pub fn info(jpeg: &[u8]) -> Option<JpegInfo> {
    smallpeg::jpeg_info(jpeg).map(|i| JpegInfo {
        width: i.width,
        height: i.height,
        components: i.components,
        progressive: i.progressive,
        orientation: i.orientation,
    })
}

/// Decode a grayscale or color JPEG to 8-bit gray or RGB pixels, as
/// stored (see `applyOrientation`). `undefined` for CMYK files, codings
/// this library does not decode, malformed input, or frames over 100
/// megapixels.
#[wasm_bindgen]
pub fn decode(jpeg: &[u8]) -> Option<Image> {
    smallpeg::decode_jpeg(jpeg).map(|d| Image {
        width: d.width,
        height: d.height,
        channels: d.channels,
        pixels: d.pixels,
    })
}

/// Turn decoded pixels upright for an EXIF orientation (1..=8; other
/// values leave them as they are). Orientations 5 to 8 swap width and
/// height.
#[wasm_bindgen(js_name = applyOrientation)]
pub fn apply_orientation(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    orientation: u8,
) -> Result<Image, JsError> {
    if !(1..=4).contains(&channels) {
        return Err(JsError::new("channels must be 1 to 4"));
    }
    check_pixels(pixels.len(), width, height, channels)?;
    let (px, w, h) =
        smallpeg::apply_orientation(pixels, width, height, channels as usize, orientation);
    Ok(Image {
        width: w,
        height: h,
        channels: channels as u8,
        pixels: px,
    })
}

/// Resize with a Lanczos3 filter. `channels` is 1 (gray), 3 (RGB) or 4
/// (any four-channel layout, each channel filtered independently).
/// `linear` filters RGB or gray in linear light, which keeps fine
/// light-and-dark detail from darkening when reducing; it assumes sRGB
/// input and is not available for 4 channels.
#[allow(non_snake_case)] // parameter names are the JavaScript ones
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn resize(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    newWidth: u32,
    newHeight: u32,
    linear: Option<bool>,
) -> Result<Vec<u8>, JsError> {
    check_pixels(pixels.len(), width, height, channels)?;
    if newWidth == 0 || newHeight == 0 {
        return Err(JsError::new("newWidth and newHeight must be at least 1"));
    }
    let linear = linear.unwrap_or(false);
    let (w, h, nw, nh) = (width, height, newWidth, newHeight);
    Ok(match (channels, linear) {
        (1, false) => smallpeg::resize_gray(pixels, w, h, nw, nh),
        (1, true) => smallpeg::resize_gray_linear(pixels, w, h, nw, nh),
        (3, false) => smallpeg::resize_rgb(pixels, w, h, nw, nh),
        (3, true) => smallpeg::resize_rgb_linear(pixels, w, h, nw, nh),
        (4, false) => smallpeg::resize_cmyk(pixels, w, h, nw, nh),
        (4, true) => return Err(JsError::new("linear resizing takes 1 or 3 channels")),
        _ => return Err(JsError::new("channels must be 1, 3 or 4")),
    })
}

/// Copy a rectangle out of pixels with any channel count. `undefined` if
/// the rectangle is empty or not entirely inside the image.
#[allow(non_snake_case)] // parameter names are the JavaScript ones
#[wasm_bindgen]
#[allow(clippy::too_many_arguments)]
pub fn crop(
    pixels: &[u8],
    width: u32,
    height: u32,
    channels: u32,
    x: u32,
    y: u32,
    cropWidth: u32,
    cropHeight: u32,
) -> Result<Option<Vec<u8>>, JsError> {
    if channels == 0 {
        return Err(JsError::new("channels must be at least 1"));
    }
    check_pixels(pixels.len(), width, height, channels)?;
    let rect = smallpeg::CropRect {
        x,
        y,
        width: cropWidth,
        height: cropHeight,
    };
    Ok(smallpeg::crop(
        pixels,
        width,
        height,
        channels as usize,
        rect,
    ))
}

/// The largest `[width, height]` that fits inside `maxWidth` x `maxHeight`
/// with the same aspect ratio. Never enlarges.
#[allow(non_snake_case)] // parameter names are the JavaScript ones
#[wasm_bindgen(js_name = fitWithin)]
pub fn fit_within(
    width: u32,
    height: u32,
    maxWidth: u32,
    maxHeight: u32,
) -> Result<Vec<u32>, JsError> {
    if width == 0 || height == 0 || maxWidth == 0 || maxHeight == 0 {
        return Err(JsError::new("all sizes must be at least 1"));
    }
    let (w, h) = smallpeg::fit_within(width, height, maxWidth, maxHeight);
    Ok(vec![w, h])
}
