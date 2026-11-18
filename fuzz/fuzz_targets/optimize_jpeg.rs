//! Coverage-guided fuzz of the hostile-input surface: no entry point that
//! takes JPEG bytes may panic, whatever the bytes. (Validity of the Some()
//! outputs is covered by the pixel-identity oracles in the test suite and
//! the third-party roundtrip harness.)
#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = smallpeg::jpeg_info(data);
    let _ = smallpeg::strip_metadata(data);
    let _ = smallpeg::optimize_jpeg(data);
    let _ = smallpeg::optimize_jpeg_baseline(data);
    // The pixel stage (IDCT, upsampling, color conversion) too. A pixel
    // budget keeps single runs well inside the fuzzer's RSS limit.
    let limits = smallpeg::Limits::default().max_pixels(1 << 22);
    // Turned output must itself be a JPEG the parser accepts.
    if let Some(up) = smallpeg::upright_jpeg_with(data, &limits) {
        assert!(smallpeg::optimize_jpeg(&up).is_some());
    }
    let _ = smallpeg::decode_jpeg_with(data, &limits);
    let _ = smallpeg::decode_cmyk_with(data, &limits);
});
