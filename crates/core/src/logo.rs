//! The logo of a floor: one call to the OpenAI image API, and the clean-up of the picture.
//!
//! This module is blocking. It uses `ureq` and no async runtime. The caller runs it in a
//! thread. It knows nothing about floors: it takes a request and gives PNG bytes. The base URL is
//! an argument, so a test can use a local fake server. The key goes in a header and nowhere else.
//! An error text never holds the key (see `redact`).

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::imageops::{self, FilterType};
use image::{ImageFormat, RgbaImage};
use serde_json::{Value, json};
use std::io::Cursor;
use std::time::Duration;

/// The base URL of the real API.
pub const DEFAULT_API_BASE: &str = "https://api.openai.com";

/// The image model when the operator did not choose one.
pub const DEFAULT_IMAGE_MODEL: &str = "gpt-image-2.6-sunburst";

/// The time limit of the whole request.
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(180);

/// The logo fits in a square of this size (in pixels).
pub const MAX_SIDE: u32 = 512;

/// A pixel with an alpha below this value becomes fully transparent.
const ALPHA_CUT: u8 = 128;

/// The most bytes that the answer of the API can have.
const MAX_ANSWER_BYTES: u64 = 64 * 1024 * 1024;

/// What one request needs.
pub struct LogoRequest<'a> {
    /// For example `https://api.openai.com`. No slash at the end is needed.
    pub api_base: &'a str,
    pub key: &'a str,
    pub model: &'a str,
    /// The subject of the logo (`[brand] logo_prompt`, or the floor name).
    pub subject: &'a str,
    pub timeout: Duration,
}

/// The full prompt: the fixed style words, and the subject.
pub fn sticker_prompt(subject: &str) -> String {
    let subject = subject.trim().trim_end_matches('.').trim_end();
    format!(
        "One die-cut sticker logo with a thick white sticker border, flat front view, bold simple emblem style, no text, no letters: {subject}. Centered, large, transparent background."
    )
}

/// Replace the key in a text. Used on each error text that can come from the network.
fn redact(text: &str, key: &str) -> String {
    if key.is_empty() {
        text.to_owned()
    } else {
        text.replace(key, "***")
    }
}

/// Ask the API for a logo and clean the picture up. The result is the bytes of the PNG file.
pub fn generate(request: &LogoRequest) -> Result<Vec<u8>, String> {
    let raw = fetch(request)?;
    clean_up(&raw)
}

/// The call to the API. The result is the bytes of the picture as the API made them.
fn fetch(request: &LogoRequest) -> Result<Vec<u8>, String> {
    let url = format!(
        "{}/v1/images/generations",
        request.api_base.trim_end_matches('/')
    );
    let body = json!({
        "model": request.model,
        "prompt": sticker_prompt(request.subject),
        "size": "1024x1024",
        "quality": "medium",
        "background": "transparent",
        "n": 1,
    })
    .to_string();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(request.timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let fail = |what: &str, error: &dyn std::fmt::Display| {
        redact(&format!("{what}: {error}"), request.key)
    };
    let mut response = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {}", request.key))
        .header("Content-Type", "application/json")
        .send(body)
        .map_err(|error| fail("the image request failed", &error))?;
    let status = response.status().as_u16();
    let text = response
        .body_mut()
        .with_config()
        .limit(MAX_ANSWER_BYTES)
        .read_to_string()
        .map_err(|error| fail("cannot read the answer of the image API", &error))?;
    if status == 401 {
        return Err("the OpenAI key was refused (status 401)".to_owned());
    }
    if !(200..300).contains(&status) {
        return Err(redact(
            &format!(
                "the image API answered with status {status}{}",
                api_message(&text)
            ),
            request.key,
        ));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|error| fail("the answer of the image API is not JSON", &error))?;
    let encoded = value["data"][0]["b64_json"]
        .as_str()
        .ok_or_else(|| "the answer of the image API has no data[0].b64_json".to_owned())?;
    STANDARD
        .decode(encoded)
        .map_err(|error| format!("the picture of the image API is not valid base64: {error}"))
}

/// The text of `error.message` in an error answer, as `: text`, cut to 300 characters.
fn api_message(text: &str) -> String {
    let message = serde_json::from_str::<Value>(text)
        .ok()
        .and_then(|value| value["error"]["message"].as_str().map(str::to_owned));
    match message {
        Some(message) => format!(": {}", message.chars().take(300).collect::<String>()),
        None => String::new(),
    }
}

/// Set alpha to 0 where it is below 128, crop to the box of the pixels that stay, scale down to fit
/// 512 x 512 (a smaller picture keeps its size), and write a PNG.
pub fn clean_up(picture: &[u8]) -> Result<Vec<u8>, String> {
    let decoded = image::load_from_memory_with_format(picture, ImageFormat::Png)
        .map_err(|error| format!("the picture is not a PNG that this app can read: {error}"))?;
    let mut rgba = decoded.to_rgba8();
    let (width, height) = rgba.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (x, y, pixel) in rgba.enumerate_pixels_mut() {
        if pixel.0[3] < ALPHA_CUT {
            pixel.0[3] = 0;
        } else {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if left > right || top > bottom {
        return Err("the picture has no visible pixel".to_owned());
    }
    let cropped =
        imageops::crop_imm(&rgba, left, top, right - left + 1, bottom - top + 1).to_image();
    let fitted = fit(cropped);
    let mut out = Cursor::new(Vec::new());
    fitted
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|error| format!("cannot write the PNG: {error}"))?;
    Ok(out.into_inner())
}

/// The most bytes that a picture file for [`import`] can have.
pub const MAX_IMPORT_BYTES: u64 = 20 * 1024 * 1024;

/// The longest side (in pixels) that a picture for [`import`] can have.
const MAX_IMPORT_SIDE: u32 = 8192;

/// Make a logo from a picture that the project has (PNG, JPEG or WebP). The colours and the
/// alpha values stay. The empty border (alpha 0) is cropped, and the picture is scaled down to
/// fit 512 x 512 (a smaller picture keeps its size). The result is the bytes of a PNG file.
pub fn import(picture: &[u8]) -> Result<Vec<u8>, String> {
    const FORMATS: &str = "The file must be a PNG, a JPEG or a WebP picture";
    let reader = || {
        image::ImageReader::new(Cursor::new(picture))
            .with_guessed_format()
            .map_err(|error| format!("cannot read the picture: {error}"))
    };
    match reader()?.format() {
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) => {}
        _ => {
            return Err(format!(
                "{FORMATS}. Convert an SVG file to a PNG file first."
            ));
        }
    }
    let (width, height) = reader()?
        .into_dimensions()
        .map_err(|error| format!("{FORMATS}: {error}"))?;
    if width > MAX_IMPORT_SIDE || height > MAX_IMPORT_SIDE {
        return Err(format!(
            "the picture is {width} x {height} pixels. The limit is {MAX_IMPORT_SIDE} for each side."
        ));
    }
    let rgba = reader()?
        .decode()
        .map_err(|error| format!("{FORMATS}: {error}"))?
        .to_rgba8();
    let (mut left, mut top, mut right, mut bottom) = (width, height, 0, 0);
    for (x, y, pixel) in rgba.enumerate_pixels() {
        if pixel.0[3] > 0 {
            left = left.min(x);
            top = top.min(y);
            right = right.max(x);
            bottom = bottom.max(y);
        }
    }
    if left > right || top > bottom {
        return Err("the picture has no visible pixel".to_owned());
    }
    let cropped =
        imageops::crop_imm(&rgba, left, top, right - left + 1, bottom - top + 1).to_image();
    let mut out = Cursor::new(Vec::new());
    fit(cropped)
        .write_to(&mut out, ImageFormat::Png)
        .map_err(|error| format!("cannot write the PNG: {error}"))?;
    Ok(out.into_inner())
}

fn fit(picture: RgbaImage) -> RgbaImage {
    let (width, height) = picture.dimensions();
    if width <= MAX_SIDE && height <= MAX_SIDE {
        return picture;
    }
    let scale = f64::from(MAX_SIDE) / f64::from(width.max(height));
    let new_width = ((f64::from(width) * scale).round() as u32).clamp(1, MAX_SIDE);
    let new_height = ((f64::from(height) * scale).round() as u32).clamp(1, MAX_SIDE);
    imageops::resize(&picture, new_width, new_height, FilterType::Lanczos3)
}
