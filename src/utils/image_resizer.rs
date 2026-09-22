//! Native image resize/downsample helpers.
//!
//! Maps to: CC `utils/imageResizer.ts`. The official implementation uses the
//! native `image-processor-napi`/sharp wrapper; this Rust port uses the `image`
//! crate to keep the same service boundary and result shape for MCP image
//! content. Analytics/debug side effects are intentionally omitted.

use crate::constants::api_limits::{
    API_IMAGE_MAX_BASE64_SIZE, IMAGE_MAX_HEIGHT, IMAGE_MAX_WIDTH, IMAGE_TARGET_RAW_SIZE,
};
use anyhow::Context;
use base64::Engine as _;
use image::{GenericImageView, ImageEncoder};
/// Maps to: CC `ImageResizeError`.
#[derive(Debug)]
pub struct ImageResizeError {
    message: String,
}

impl ImageResizeError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for ImageResizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ImageResizeError {}

/// Maps to: CC `ImageDimensions`.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageDimensions {
    pub original_width: Option<u32>,
    pub original_height: Option<u32>,
    pub display_width: Option<u32>,
    pub display_height: Option<u32>,
}

/// Maps to: CC `ResizeResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResizeResult {
    pub buffer: Vec<u8>,
    /// API MIME type (`image/png`, `image/jpeg`, ...). CC stores the extension
    /// in `mediaType` and prefixes it at the call site; Rust stores the final
    /// MIME string because MCP block builders consume MIME types directly.
    pub media_type: String,
    pub dimensions: Option<ImageDimensions>,
}

/// Maps to: CC `CompressedImageResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompressedImageResult {
    pub base64: String,
    pub media_type: String,
    pub original_size: usize,
}

fn normalize_ext(ext: &str) -> String {
    match ext.trim().to_ascii_lowercase().as_str() {
        "jpg" => "jpeg".to_string(),
        "jpeg" => "jpeg".to_string(),
        "png" => "png".to_string(),
        "gif" => "gif".to_string(),
        "webp" => "webp".to_string(),
        other if !other.is_empty() => other.to_string(),
        _ => "png".to_string(),
    }
}

fn ext_from_mime_or_ext(value: Option<&str>) -> String {
    value
        .and_then(|value| value.split('/').nth(1).or(Some(value)))
        .map(normalize_ext)
        .unwrap_or_else(|| "jpeg".to_string())
}

fn mime_from_ext(ext: &str) -> String {
    format!("image/{}", normalize_ext(ext))
}

/// Maps to: CC `utils/imageResizer.ts#detectImageFormatFromBuffer`.
pub fn detect_image_format_from_buffer(bytes: &[u8]) -> String {
    if bytes.len() < 4 {
        return "image/png".to_string();
    }
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        return "image/png".to_string();
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg".to_string();
    }
    if bytes.starts_with(b"GIF") {
        return "image/gif".to_string();
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return "image/webp".to_string();
    }
    "image/png".to_string()
}

fn decode_base64_like_node(data: &str) -> Result<Vec<u8>, ImageResizeError> {
    let mut normalized = Vec::with_capacity(data.len());
    for byte in data.bytes() {
        match byte {
            b'=' => break,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'+' | b'/' => normalized.push(byte),
            b'-' => normalized.push(b'+'),
            b'_' => normalized.push(b'/'),
            _ => {}
        }
    }
    if normalized.len() % 4 == 1 {
        normalized.pop();
    }
    let engine = base64::engine::general_purpose::GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        base64::engine::general_purpose::GeneralPurposeConfig::new()
            .with_encode_padding(false)
            .with_decode_padding_mode(base64::engine::DecodePaddingMode::RequireNone)
            .with_decode_allow_trailing_bits(true),
    );
    engine
        .decode(normalized)
        .map_err(|error| ImageResizeError::new(error.to_string()))
}

fn png_dimensions_over_limit(bytes: &[u8]) -> bool {
    bytes.len() >= 24
        && bytes.starts_with(&[0x89, b'P', b'N', b'G'])
        && (u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]) > IMAGE_MAX_WIDTH
            || u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) > IMAGE_MAX_HEIGHT)
}

fn base64_size(raw_size: usize) -> usize {
    (((raw_size as f64) * 4.0) / 3.0).ceil() as usize
}

fn constrain_dimensions(mut width: u32, mut height: u32) -> (u32, u32) {
    if width > IMAGE_MAX_WIDTH {
        height = ((height as u64 * IMAGE_MAX_WIDTH as u64 + (width as u64 / 2)) / width as u64)
            .max(1) as u32;
        width = IMAGE_MAX_WIDTH;
    }
    if height > IMAGE_MAX_HEIGHT {
        width = ((width as u64 * IMAGE_MAX_HEIGHT as u64 + (height as u64 / 2)) / height as u64)
            .max(1) as u32;
        height = IMAGE_MAX_HEIGHT;
    }
    (width.max(1), height.max(1))
}

fn encode_png(image: &image::DynamicImage) -> anyhow::Result<Vec<u8>> {
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut output = Vec::new();
    image::codecs::png::PngEncoder::new(&mut output)
        .write_image(
            rgba.as_raw(),
            width,
            height,
            image::ExtendedColorType::Rgba8,
        )
        .context("encode png")?;
    Ok(output)
}

fn encode_jpeg(image: &image::DynamicImage, quality: u8) -> anyhow::Result<Vec<u8>> {
    let rgb = image.to_rgb8();
    let (width, height) = rgb.dimensions();
    let mut output = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, quality).encode(
        rgb.as_raw(),
        width,
        height,
        image::ExtendedColorType::Rgb8,
    )?;
    Ok(output)
}

fn decode_image_without_rust_limits(
    bytes: &[u8],
) -> Result<image::DynamicImage, image::ImageError> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    reader.limits(image::Limits::no_limits());
    reader.decode()
}

fn encode_in_source_format(
    image: &image::DynamicImage,
    extension: &str,
) -> anyhow::Result<Vec<u8>> {
    match normalize_ext(extension).as_str() {
        "png" => encode_png(image),
        "jpeg" => encode_jpeg(image, 80),
        "gif" => {
            let mut output = std::io::Cursor::new(Vec::new());
            image.write_to(&mut output, image::ImageFormat::Gif)?;
            Ok(output.into_inner())
        }
        "webp" => {
            let mut output = std::io::Cursor::new(Vec::new());
            image.write_to(&mut output, image::ImageFormat::WebP)?;
            Ok(output.into_inner())
        }
        _ => encode_png(image),
    }
}

fn resize_without_enlargement(
    image: &image::DynamicImage,
    width: u32,
    height: u32,
) -> image::DynamicImage {
    let (original_width, original_height) = image.dimensions();
    if original_width <= width && original_height <= height {
        image.clone()
    } else {
        image.resize(
            width.max(1),
            height.max(1),
            image::imageops::FilterType::Lanczos3,
        )
    }
}

fn dimensions(
    original_width: u32,
    original_height: u32,
    width: u32,
    height: u32,
) -> ImageDimensions {
    ImageDimensions {
        original_width: Some(original_width),
        original_height: Some(original_height),
        display_width: Some(width),
        display_height: Some(height),
    }
}

fn passthrough_result(
    bytes: &[u8],
    ext: &str,
    dimensions: Option<ImageDimensions>,
) -> ResizeResult {
    ResizeResult {
        buffer: bytes.to_vec(),
        media_type: mime_from_ext(ext),
        dimensions,
    }
}

/// Maps to: CC `utils/imageResizer.ts:383-432` — the `catch` arm shared by every
/// failure raised inside `maybeResizeAndDownsampleImageBuffer`'s `try` block,
/// including the `toBuffer()` encode calls.
fn resize_fallback(
    image_buffer: &[u8],
    original_size: usize,
) -> Result<ResizeResult, ImageResizeError> {
    let detected_ext = detect_image_format_from_buffer(image_buffer)
        .strip_prefix("image/")
        .unwrap_or("png")
        .to_string();
    let over_dim = png_dimensions_over_limit(image_buffer);
    let encoded_size = base64_size(original_size);
    if encoded_size <= API_IMAGE_MAX_BASE64_SIZE && !over_dim {
        return Ok(passthrough_result(image_buffer, &detected_ext, None));
    }
    let message = if over_dim {
        format!(
            "Unable to resize image — dimensions exceed the {IMAGE_MAX_WIDTH}x{IMAGE_MAX_HEIGHT}px limit and image processing failed. Please resize the image to reduce its pixel dimensions."
        )
    } else {
        format!(
            "Unable to resize image ({} raw, {} base64). The image exceeds the 5MB API limit and compression failed. Please resize the image manually or use a smaller image.",
            crate::utils::format::format_file_size(original_size as u64),
            crate::utils::format::format_file_size(encoded_size as u64),
        )
    };
    Err(ImageResizeError::new(message))
}

/// Maps to: CC `maybeResizeAndDownsampleImageBuffer(...)`.
pub fn maybe_resize_and_downsample_image_buffer(
    image_buffer: &[u8],
    original_size: usize,
    ext: &str,
) -> Result<ResizeResult, ImageResizeError> {
    if image_buffer.is_empty() {
        return Err(ImageResizeError::new("Image file is empty (0 bytes)"));
    }

    match try_resize_and_downsample(image_buffer, original_size, ext) {
        Ok(result) => Ok(result),
        Err(_error) => resize_fallback(image_buffer, original_size),
    }
}

/// Maps to: CC `utils/imageResizer.ts:181-382` — the `try` block body.
fn try_resize_and_downsample(
    image_buffer: &[u8],
    original_size: usize,
    ext: &str,
) -> anyhow::Result<ResizeResult> {
    let fallback_ext = normalize_ext(ext);
    let decoded = decode_image_without_rust_limits(image_buffer)?;

    let detected_ext = image::guess_format(image_buffer)
        .ok()
        .and_then(|format| match format {
            image::ImageFormat::Jpeg => Some("jpeg".to_string()),
            image::ImageFormat::Png => Some("png".to_string()),
            image::ImageFormat::Gif => Some("gif".to_string()),
            image::ImageFormat::WebP => Some("webp".to_string()),
            _ => None,
        })
        .unwrap_or(fallback_ext);
    let (original_width, original_height) = decoded.dimensions();

    if original_size <= IMAGE_TARGET_RAW_SIZE
        && original_width <= IMAGE_MAX_WIDTH
        && original_height <= IMAGE_MAX_HEIGHT
    {
        return Ok(passthrough_result(
            image_buffer,
            &detected_ext,
            Some(dimensions(
                original_width,
                original_height,
                original_width,
                original_height,
            )),
        ));
    }

    let needs_dimension_resize =
        original_width > IMAGE_MAX_WIDTH || original_height > IMAGE_MAX_HEIGHT;
    let is_png = detected_ext == "png";

    if !needs_dimension_resize && original_size > IMAGE_TARGET_RAW_SIZE {
        if is_png {
            let png = encode_png(&decoded)?;
            if png.len() <= IMAGE_TARGET_RAW_SIZE {
                return Ok(ResizeResult {
                    buffer: png,
                    media_type: mime_from_ext("png"),
                    dimensions: Some(dimensions(
                        original_width,
                        original_height,
                        original_width,
                        original_height,
                    )),
                });
            }
        }
        for quality in [80, 60, 40, 20] {
            let jpeg = encode_jpeg(&decoded, quality)?;
            if jpeg.len() <= IMAGE_TARGET_RAW_SIZE {
                return Ok(ResizeResult {
                    buffer: jpeg,
                    media_type: mime_from_ext("jpeg"),
                    dimensions: Some(dimensions(
                        original_width,
                        original_height,
                        original_width,
                        original_height,
                    )),
                });
            }
        }
    }

    let (width, height) = constrain_dimensions(original_width, original_height);
    let resized = resize_without_enlargement(&decoded, width, height);
    let resized_in_source_format = encode_in_source_format(&resized, &detected_ext)?;
    if resized_in_source_format.len() <= IMAGE_TARGET_RAW_SIZE {
        return Ok(ResizeResult {
            buffer: resized_in_source_format,
            media_type: mime_from_ext(&detected_ext),
            dimensions: Some(dimensions(original_width, original_height, width, height)),
        });
    }

    if is_png {
        let png = encode_png(&resized)?;
        if png.len() <= IMAGE_TARGET_RAW_SIZE {
            return Ok(ResizeResult {
                buffer: png,
                media_type: mime_from_ext("png"),
                dimensions: Some(dimensions(original_width, original_height, width, height)),
            });
        }
    }

    for quality in [80, 60, 40, 20] {
        let jpeg = encode_jpeg(&resized, quality)?;
        if jpeg.len() <= IMAGE_TARGET_RAW_SIZE {
            return Ok(ResizeResult {
                buffer: jpeg,
                media_type: mime_from_ext("jpeg"),
                dimensions: Some(dimensions(original_width, original_height, width, height)),
            });
        }
    }

    let smaller_width = width.clamp(1, 1000);
    let smaller_height = ((height as u64 * smaller_width as u64 + (width as u64 / 2))
        / width.max(1) as u64)
        .max(1) as u32;
    let smaller = resize_without_enlargement(&decoded, smaller_width, smaller_height);
    let jpeg = encode_jpeg(&smaller, 20)?;
    Ok(ResizeResult {
        buffer: jpeg,
        media_type: mime_from_ext("jpeg"),
        dimensions: Some(dimensions(
            original_width,
            original_height,
            smaller_width,
            smaller_height,
        )),
    })
}

/// Maps to: CC `utils/imageResizer.ts:632-645`
/// `createCompressedImageResult`.
fn create_compressed_image_result(
    buffer: Vec<u8>,
    ext: &str,
    original_size: usize,
) -> CompressedImageResult {
    CompressedImageResult {
        base64: base64::engine::general_purpose::STANDARD.encode(buffer),
        media_type: mime_from_ext(ext),
        original_size,
    }
}

/// Maps to: CC `utils/imageResizer.ts:548-576` — the `catch` arm shared by every
/// failure raised inside `compressImageBuffer`'s `try` block. The passthrough is
/// only reachable for pre-`originalSize <= maxBytes` failures; encode failures
/// past that check always surface the "Unable to compress image" error.
fn compress_fallback(
    image_buffer: &[u8],
    max_bytes: usize,
) -> Result<CompressedImageResult, ImageResizeError> {
    let original_size = image_buffer.len();
    if original_size <= max_bytes {
        let detected = detect_image_format_from_buffer(image_buffer);
        let extension = detected.strip_prefix("image/").unwrap_or("png");
        return Ok(create_compressed_image_result(
            image_buffer.to_vec(),
            extension,
            original_size,
        ));
    }
    Err(ImageResizeError::new(format!(
        "Unable to compress image ({}) to fit within {}. Please use a smaller image.",
        crate::utils::format::format_file_size(original_size as u64),
        crate::utils::format::format_file_size(max_bytes as u64),
    )))
}

/// Maps to: CC `compressImageBuffer(...)`.
pub fn compress_image_buffer(
    image_buffer: &[u8],
    max_bytes: usize,
    original_media_type: Option<&str>,
) -> Result<CompressedImageResult, ImageResizeError> {
    match try_compress_image_buffer(image_buffer, max_bytes, original_media_type) {
        Ok(result) => Ok(result),
        Err(_error) => compress_fallback(image_buffer, max_bytes),
    }
}

/// Maps to: CC `utils/imageResizer.ts:507-547` — the `try` block body.
fn try_compress_image_buffer(
    image_buffer: &[u8],
    max_bytes: usize,
    original_media_type: Option<&str>,
) -> anyhow::Result<CompressedImageResult> {
    let original_size = image_buffer.len();
    let fallback_ext = ext_from_mime_or_ext(original_media_type);
    let decoded = decode_image_without_rust_limits(image_buffer)?;
    let format_ext = image::guess_format(image_buffer)
        .ok()
        .and_then(|format| match format {
            image::ImageFormat::Jpeg => Some("jpeg".to_string()),
            image::ImageFormat::Png => Some("png".to_string()),
            image::ImageFormat::Gif => Some("gif".to_string()),
            image::ImageFormat::WebP => Some("webp".to_string()),
            _ => None,
        })
        .unwrap_or(fallback_ext);

    if original_size <= max_bytes {
        return Ok(create_compressed_image_result(
            image_buffer.to_vec(),
            &format_ext,
            original_size,
        ));
    }

    let (original_width, original_height) = decoded.dimensions();
    for scaling_factor in [1.0, 0.75, 0.5, 0.25] {
        let width = ((original_width as f64 * scaling_factor).round() as u32).max(1);
        let height = ((original_height as f64 * scaling_factor).round() as u32).max(1);
        let candidate = resize_without_enlargement(&decoded, width, height);
        let encoded = match format_ext.as_str() {
            "png" => encode_png(&candidate),
            "jpeg" => encode_jpeg(&candidate, 80),
            _ => encode_in_source_format(&candidate, &format_ext),
        }?;
        if encoded.len() <= max_bytes {
            return Ok(create_compressed_image_result(
                encoded,
                &format_ext,
                original_size,
            ));
        }
    }

    if format_ext == "png" {
        let candidate = resize_without_enlargement(&decoded, 800, 800);
        let encoded = encode_png(&candidate)?;
        if encoded.len() <= max_bytes {
            return Ok(create_compressed_image_result(
                encoded,
                "png",
                original_size,
            ));
        }
    }

    let moderate = resize_without_enlargement(&decoded, 600, 600);
    let jpeg = encode_jpeg(&moderate, 50)?;
    if jpeg.len() <= max_bytes {
        return Ok(create_compressed_image_result(jpeg, "jpeg", original_size));
    }

    let ultra = resize_without_enlargement(&decoded, 400, 400);
    let jpeg = encode_jpeg(&ultra, 20)?;
    Ok(create_compressed_image_result(jpeg, "jpeg", original_size))
}

/// Maps to: CC `compressImageBufferWithTokenLimit(...)`.
pub fn compress_image_buffer_with_token_limit(
    image_buffer: &[u8],
    max_tokens: f64,
    original_media_type: Option<&str>,
) -> Result<CompressedImageResult, ImageResizeError> {
    let max_base64_chars = (max_tokens / 0.125).floor() as usize;
    let max_bytes = ((max_base64_chars as f64) * 0.75).floor() as usize;
    compress_image_buffer(image_buffer, max_bytes, original_media_type)
}

/// Maps to: CC `createImageMetadataText(...)`.
pub fn create_image_metadata_text(
    dims: &ImageDimensions,
    source_path: Option<&str>,
) -> Option<String> {
    let original_width = dims.original_width.filter(|w| *w > 0);
    let original_height = dims.original_height.filter(|h| *h > 0);
    let display_width = dims.display_width.filter(|w| *w > 0);
    let display_height = dims.display_height.filter(|h| *h > 0);

    let (Some(original_width), Some(original_height), Some(display_width), Some(display_height)) = (
        original_width,
        original_height,
        display_width,
        display_height,
    ) else {
        return source_path.map(|path| format!("[Image source: {path}]"));
    };

    let was_resized = original_width != display_width || original_height != display_height;
    if !was_resized && source_path.is_none() {
        return None;
    }

    let mut parts = Vec::new();
    if let Some(path) = source_path {
        parts.push(format!("source: {path}"));
    }
    if was_resized {
        let scale_factor = f64::from(original_width) / f64::from(display_width);
        parts.push(format!(
            "original {original_width}x{original_height}, displayed at {display_width}x{display_height}. Multiply coordinates by {scale_factor:.2} to map to original image."
        ));
    }
    Some(format!("[Image: {}]", parts.join(", ")))
}

/// Maps to: CC `utils/imageResizer.ts#compressImageBlock`.
pub fn compress_image_block(
    image_block: &serde_json::Value,
    max_bytes: usize,
) -> Result<serde_json::Value, ImageResizeError> {
    let source = image_block.get("source").ok_or_else(|| {
        ImageResizeError::new("Cannot read properties of undefined (reading 'type')")
    })?;
    if source.is_null() {
        return Err(ImageResizeError::new(
            "Cannot read properties of null (reading 'type')",
        ));
    }
    if source.get("type").and_then(serde_json::Value::as_str) != Some("base64") {
        return Ok(image_block.clone());
    }
    let data = source
        .get("data")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ImageResizeError::new(
                "The first argument must be of type string or an instance of Buffer, ArrayBuffer, or Array or an Array-like Object",
            )
        })?;
    let bytes = decode_base64_like_node(data)?;
    if bytes.len() <= max_bytes {
        return Ok(image_block.clone());
    }
    let compressed = compress_image_buffer(&bytes, max_bytes, None)?;
    Ok(serde_json::json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": compressed.media_type,
            "data": compressed.base64,
        }
    }))
}

/// Convenience wrapper for MCP base64 image content.
pub fn maybe_resize_and_downsample_image_base64(
    data: &str,
    mime_type: Option<&str>,
) -> Result<ResizeResult, ImageResizeError> {
    let bytes = decode_base64_like_node(data)?;
    let extension = mime_type
        .and_then(|value| value.split('/').nth(1))
        .map(normalize_ext)
        .unwrap_or_else(|| "png".to_string());
    maybe_resize_and_downsample_image_buffer(&bytes, bytes.len(), &extension)
}

pub fn resize_result_base64(result: &ResizeResult) -> String {
    base64::engine::general_purpose::STANDARD.encode(&result.buffer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageEncoder;

    fn png_base64(width: u32, height: u32) -> String {
        let rgba = image::RgbaImage::from_pixel(width, height, image::Rgba([16, 32, 48, 255]));
        let mut bytes = Vec::new();
        image::codecs::png::PngEncoder::new(&mut bytes)
            .write_image(
                rgba.as_raw(),
                width,
                height,
                image::ExtendedColorType::Rgba8,
            )
            .unwrap();
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    #[test]
    fn short_format_signatures_use_official_png_fallback() {
        assert_eq!(
            detect_image_format_from_buffer(&[0xff, 0xd8, 0xff]),
            "image/png"
        );
        assert_eq!(detect_image_format_from_buffer(b"GIF"), "image/png");
        assert_eq!(
            detect_image_format_from_buffer(&[0xff, 0xd8, 0xff, 0x00]),
            "image/jpeg"
        );
    }

    #[test]
    fn maybe_resize_and_downsample_image_base64_resizes_oversized_png_dimensions() {
        let resized =
            maybe_resize_and_downsample_image_base64(&png_base64(2500, 500), Some("image/png"))
                .expect("resize image");

        assert_eq!(resized.media_type, "image/png");
        let image = image::load_from_memory(&resized.buffer).expect("decode resized image");
        assert_eq!(image.dimensions(), (2000, 400));
        assert_eq!(
            resized.dimensions,
            Some(ImageDimensions {
                original_width: Some(2500),
                original_height: Some(500),
                display_width: Some(2000),
                display_height: Some(400),
            })
        );
    }

    #[test]
    fn compress_image_block_returns_small_base64_image_unchanged_like_official() {
        let block = serde_json::json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": "image/png",
                "data": png_base64(1, 1),
            }
        });
        let compressed = compress_image_block(&block, 300).expect("small image fits byte budget");
        assert_eq!(compressed, block);
    }

    #[test]
    fn compress_image_block_throws_for_missing_source_or_base64_data() {
        assert_eq!(
            compress_image_block(&serde_json::json!({"type": "image"}), 300)
                .unwrap_err()
                .to_string(),
            "Cannot read properties of undefined (reading 'type')"
        );
        assert!(
            compress_image_block(
                &serde_json::json!({"type": "image", "source": {"type": "base64"}}),
                300
            )
            .is_err()
        );
    }

    #[test]
    fn base64_decode_matches_node_buffer_forgiving_padding_and_url_alphabet() {
        assert_eq!(decode_base64_like_node("A").unwrap(), b"");
        assert_eq!(decode_base64_like_node("AA=A").unwrap(), [0]);
        assert_eq!(decode_base64_like_node("SGV sbG8===").unwrap(), b"Hello");
        assert_eq!(decode_base64_like_node("_-").unwrap(), [255]);
        assert_eq!(
            decode_base64_like_node("abcde").unwrap(),
            [0x69, 0xb7, 0x1d]
        );
    }

    #[test]
    fn maybe_resize_and_downsample_image_buffer_rejects_empty_buffer_like_official() {
        let error = maybe_resize_and_downsample_image_buffer(&[], 0, "png")
            .expect_err("empty image should fail");
        assert_eq!(error.to_string(), "Image file is empty (0 bytes)");
    }

    fn png_header(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        bytes.extend_from_slice(&[0, 0, 0, 13]);
        bytes.extend_from_slice(b"IHDR");
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[test]
    fn resize_fallback_passes_through_when_base64_fits_and_dimensions_are_unknown() {
        let jpeg = [0xff, 0xd8, 0xff, 0x00, 0x01, 0x02];
        let result = resize_fallback(&jpeg, jpeg.len()).expect("small buffer passes through");

        assert_eq!(result.buffer, jpeg);
        assert_eq!(result.media_type, "image/jpeg");
        assert_eq!(result.dimensions, None);
    }

    #[test]
    fn resize_fallback_passes_through_at_the_base64_limit_but_not_past_it() {
        let jpeg = [0xff, 0xd8, 0xff, 0x00];
        assert!(resize_fallback(&jpeg, IMAGE_TARGET_RAW_SIZE).is_ok());

        let error = resize_fallback(&jpeg, IMAGE_TARGET_RAW_SIZE + 1)
            .expect_err("one byte past the raw target exceeds the 5MB base64 limit");
        assert!(
            error.to_string().starts_with("Unable to resize image ("),
            "unexpected message: {error}"
        );
    }

    #[test]
    fn resize_fallback_rejects_oversized_png_dimensions_even_when_base64_fits() {
        let header = png_header(IMAGE_MAX_WIDTH + 1, 10);
        let error = resize_fallback(&header, header.len())
            .expect_err("PNG header past the dimension cap must not pass through");
        assert_eq!(
            error.to_string(),
            format!(
                "Unable to resize image — dimensions exceed the {IMAGE_MAX_WIDTH}x{IMAGE_MAX_HEIGHT}px limit and image processing failed. Please resize the image to reduce its pixel dimensions."
            )
        );

        let within = png_header(IMAGE_MAX_WIDTH, IMAGE_MAX_HEIGHT);
        assert!(resize_fallback(&within, within.len()).is_ok());
    }

    #[test]
    fn compress_fallback_passes_through_only_within_the_byte_budget() {
        let png = [0x89, b'P', b'N', b'G', 0x00];
        let result = compress_fallback(&png, png.len()).expect("buffer within budget");
        assert_eq!(result.media_type, "image/png");
        assert_eq!(result.original_size, png.len());

        let error = compress_fallback(&png, png.len() - 1)
            .expect_err("buffer past the budget cannot pass through");
        assert!(
            error.to_string().starts_with("Unable to compress image ("),
            "unexpected message: {error}"
        );
    }
}
