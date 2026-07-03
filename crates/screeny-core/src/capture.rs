//! Screen capture via `xcap` (Windows DXGI / macOS CoreGraphics / Linux X11).
//! All functions here are blocking — call them from `spawn_blocking`.

use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, RgbaImage};

use crate::config::ImageFormat;
use crate::error::{CoreError, Result};
use crate::pipeline::Frame;

/// Capture the primary monitor (falls back to the first monitor).
pub fn capture_primary() -> Result<Frame> {
    let monitors =
        xcap::Monitor::all().map_err(|e| CoreError::Capture(format!("list monitors: {e}")))?;
    let monitor = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| CoreError::Capture("no monitors found".into()))?;

    let name = monitor.name().unwrap_or_else(|_| "unknown".into());
    let image = monitor
        .capture_image()
        .map_err(|e| CoreError::Capture(format!("capture monitor '{name}': {e}")))?;

    if looks_blank(&image) {
        return Err(CoreError::Capture(
            "capture came back blank (missing screen permission?)".into(),
        ));
    }

    Ok(Frame {
        image,
        monitor: name,
    })
}

/// Encode a raw RGBA frame to PNG or JPEG bytes.
pub fn encode(image: &RgbaImage, format: ImageFormat, jpeg_quality: u8) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    match format {
        ImageFormat::Png => {
            DynamicImage::ImageRgba8(image.clone())
                .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
                .map_err(|e| CoreError::Encode(e.to_string()))?;
        }
        ImageFormat::Jpeg => {
            let rgb = DynamicImage::ImageRgba8(image.clone()).to_rgb8();
            let encoder = JpegEncoder::new_with_quality(&mut out, jpeg_quality.clamp(1, 100));
            rgb.write_with_encoder(encoder)
                .map_err(|e| CoreError::Encode(e.to_string()))?;
        }
    }
    Ok(out)
}

/// Shrink an encoded image so its longest edge is at most `max_edge` before
/// sending it to a vision model — smaller payloads, much faster inference.
/// Returns re-encoded JPEG bytes (or the original bytes if already small).
pub fn downscale_for_llm(bytes: &[u8], max_edge: u32) -> Result<Vec<u8>> {
    let img = image::load_from_memory(bytes).map_err(|e| CoreError::Encode(e.to_string()))?;
    let (w, h) = (img.width(), img.height());
    if w.max(h) <= max_edge {
        return Ok(bytes.to_vec());
    }
    let resized = img.resize(max_edge, max_edge, image::imageops::FilterType::Triangle);
    let mut out = Vec::new();
    let encoder = JpegEncoder::new_with_quality(&mut out, 80);
    resized
        .to_rgb8()
        .write_with_encoder(encoder)
        .map_err(|e| CoreError::Encode(e.to_string()))?;
    Ok(out)
}

/// Detect the all-black frames macOS produces when Screen Recording permission
/// is missing. Samples a sparse grid instead of every pixel.
fn looks_blank(image: &RgbaImage) -> bool {
    let (w, h) = image.dimensions();
    if w == 0 || h == 0 {
        return true;
    }
    let step_x = (w / 16).max(1);
    let step_y = (h / 16).max(1);
    let mut samples = 0u32;
    let mut black = 0u32;
    let mut y = 0;
    while y < h {
        let mut x = 0;
        while x < w {
            let p = image.get_pixel(x, y);
            samples += 1;
            if p[0] < 3 && p[1] < 3 && p[2] < 3 {
                black += 1;
            }
            x += step_x;
        }
        y += step_y;
    }
    black == samples
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_jpeg_and_png_produce_nonempty_output() {
        let img = RgbaImage::from_fn(64, 48, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 5) as u8, 128, 255])
        });
        let jpeg = encode(&img, ImageFormat::Jpeg, 80).unwrap();
        let png = encode(&img, ImageFormat::Png, 80).unwrap();
        assert!(!jpeg.is_empty());
        assert!(!png.is_empty());
        // JPEG magic bytes / PNG signature
        assert_eq!(&jpeg[..2], &[0xFF, 0xD8]);
        assert_eq!(&png[..4], &[0x89, b'P', b'N', b'G']);
    }

    #[test]
    fn downscale_shrinks_large_images_and_passes_small_ones() {
        let large = RgbaImage::from_pixel(2560, 1440, image::Rgba([120, 120, 120, 255]));
        let bytes = encode(&large, ImageFormat::Jpeg, 80).unwrap();
        let shrunk = downscale_for_llm(&bytes, 1280).unwrap();
        let reloaded = image::load_from_memory(&shrunk).unwrap();
        assert_eq!(reloaded.width().max(reloaded.height()), 1280);

        let small = RgbaImage::from_pixel(640, 480, image::Rgba([9, 9, 9, 255]));
        let small_bytes = encode(&small, ImageFormat::Jpeg, 80).unwrap();
        assert_eq!(downscale_for_llm(&small_bytes, 1280).unwrap(), small_bytes);
    }

    #[test]
    fn blank_detection_flags_black_frames_only() {
        let black = RgbaImage::from_pixel(100, 100, image::Rgba([0, 0, 0, 255]));
        assert!(looks_blank(&black));
        // (0,0) is always on the sampling grid
        let mut almost = RgbaImage::from_pixel(100, 100, image::Rgba([0, 0, 0, 255]));
        almost.put_pixel(0, 0, image::Rgba([200, 10, 10, 255]));
        assert!(!looks_blank(&almost));
    }
}
