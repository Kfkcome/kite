// Adapted from DEEIX-AI/Winc crates/winc-preview/src/image/imageio.rs (Apache-2.0).
//! Downscaled image decoding: on macOS, JPEGs go through ImageIO's thumbnail API so
//! multi-megapixel photos decode directly at the target size instead of allocating a full-size
//! RGBA buffer first. Other formats and platforms fall back to the `image` crate.

use image::DynamicImage;
#[cfg(target_os = "macos")]
use image::ImageFormat;

/// Decodes `bytes` into an image whose longer edge is at most `max_edge` pixels.
///
/// # Errors
///
/// Returns an error message when the bytes cannot be decoded as a supported image.
pub fn decode_max_edge(bytes: &[u8], max_edge: u32) -> Result<DynamicImage, String> {
    #[cfg(target_os = "macos")]
    if max_edge > 0
        && matches!(image::guess_format(bytes), Ok(ImageFormat::Jpeg))
        && let Some(img) = macos::decode_jpeg_scaled(bytes, max_edge)
    {
        return Ok(DynamicImage::ImageRgba8(img));
    }

    let decoded =
        image::load_from_memory(bytes).map_err(|e| format!("Cannot decode the wallpaper: {e}"))?;
    if max_edge > 0 && (decoded.width() > max_edge || decoded.height() > max_edge) {
        Ok(decoded.thumbnail(max_edge, max_edge))
    } else {
        Ok(decoded)
    }
}

#[cfg(target_os = "macos")]
#[allow(unsafe_code)]
mod macos {
    use std::ptr::NonNull;

    use image::RgbaImage;
    use objc2_core_foundation::{
        CFBoolean, CFData, CFDictionary, CFNumber, CFRetained, CFString, CFType, CGPoint, CGRect,
        CGSize,
    };
    use objc2_core_graphics::{
        CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
        kCGColorSpaceSRGB,
    };

    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        fn CGImageSourceCreateWithData(
            data: &CFData,
            options: Option<&CFDictionary>,
        ) -> Option<NonNull<CFType>>;
        fn CGImageSourceCreateThumbnailAtIndex(
            source: &CFType,
            index: usize,
            options: Option<&CFDictionary>,
        ) -> Option<NonNull<CGImage>>;
        static kCGImageSourceCreateThumbnailFromImageAlways: &'static CFString;
        static kCGImageSourceThumbnailMaxPixelSize: &'static CFString;
        static kCGImageSourceShouldCacheImmediately: &'static CFString;
        static kCGImageSourceCreateThumbnailWithTransform: &'static CFString;
    }

    /// Decodes a JPEG via macOS ImageIO directly into an sRGB RGBA8 buffer with its longer edge
    /// capped at `max_edge`. Returns `None` on any decode failure so the caller can fall back.
    pub(super) fn decode_jpeg_scaled(bytes: &[u8], max_edge: u32) -> Option<RgbaImage> {
        let data = CFData::from_bytes(bytes);
        // SAFETY: `data` is a valid CFData; the returned CGImageSource follows the Create Rule.
        let source = unsafe { CFRetained::from_raw(CGImageSourceCreateWithData(&data, None)?) };

        let max = CFNumber::new_i32(i32::try_from(max_edge).ok()?);
        let yes = CFBoolean::new(true);
        // SAFETY: ImageIO global constant keys remain valid for the process lifetime.
        let keys: [&CFString; 4] = unsafe {
            [
                kCGImageSourceCreateThumbnailFromImageAlways,
                kCGImageSourceThumbnailMaxPixelSize,
                kCGImageSourceShouldCacheImmediately,
                kCGImageSourceCreateThumbnailWithTransform,
            ]
        };
        let values: [&CFType; 4] = [yes.as_ref(), max.as_ref(), yes.as_ref(), yes.as_ref()];
        let options = CFDictionary::from_slices(&keys, &values);
        // SAFETY: `source` and `options` are valid CF objects; returned CGImage follows Create Rule.
        let image = unsafe {
            CFRetained::from_raw(CGImageSourceCreateThumbnailAtIndex(
                &source,
                0,
                Some(options.as_opaque()),
            )?)
        };

        let width = CGImage::width(Some(&image));
        let height = CGImage::height(Some(&image));
        if width == 0 || height == 0 {
            return None;
        }
        // SAFETY: `kCGColorSpaceSRGB` is a CoreGraphics global constant string.
        let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
        let mut rgba = vec![0u8; width.checked_mul(height)?.checked_mul(4)?];
        // SAFETY: `rgba` is `width * height * 4` bytes and lives until `context` is dropped.
        let context = unsafe {
            CGBitmapContextCreate(
                rgba.as_mut_ptr().cast(),
                width,
                height,
                8,
                width * 4,
                Some(&space),
                CGImageAlphaInfo::NoneSkipLast.0,
            )
        }?;
        let rect = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        );
        CGContext::draw_image(Some(&context), rect, Some(&image));
        CGContext::flush(Some(&context));
        drop(context);
        for pixel in rgba.as_chunks_mut::<4>().0 {
            pixel[3] = 255;
        }
        RgbaImage::from_raw(
            u32::try_from(width).ok()?,
            u32::try_from(height).ok()?,
            rgba,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn jpeg_decodes_at_or_below_max_edge() {
        let b = &crate::wallpaper_gallery::BUILTINS[0];
        let img = decode_max_edge(b.bytes, 300).unwrap();
        assert!(img.width().max(img.height()) <= 300);
        assert!(img.width() > 0 && img.height() > 0);
    }
}
