//! The logo pipeline: the request to the image API (a local fake server), and the clean-up of the
//! picture. No test calls the real API.

mod fake_api;

use fake_api::{FakeApi, png_with, png_with_rect};
use serde_json::Value;
use std::time::Duration;
use tower_core::logo::{self, LogoRequest};

const KEY: &str = "sk-test-SECRET-key-0123";

fn request<'a>(api: &'a FakeApi, subject: &'a str) -> LogoRequest<'a> {
    LogoRequest {
        api_base: &api.base,
        key: KEY,
        model: "model-x",
        subject,
        timeout: Duration::from_secs(10),
    }
}

fn decode(png: &[u8]) -> image::RgbaImage {
    image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .unwrap()
        .to_rgba8()
}

#[test]
fn the_request_has_the_fields_of_the_contract_and_the_picture_comes_back() {
    let api = FakeApi::start();
    api.reply_picture(&png_with_rect(64, 64, 8, 8, 39, 23));
    let png = logo::generate(&request(
        &api,
        "A golden honeycomb hexagon with a small bee.",
    ))
    .unwrap();

    let sent = api.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].request_line, "POST /v1/images/generations HTTP/1.1");
    assert_eq!(
        sent[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    let body: Value = serde_json::from_str(&sent[0].body).unwrap();
    assert_eq!(body["model"], "model-x");
    assert_eq!(body["size"], "1024x1024");
    assert_eq!(body["quality"], "medium");
    assert_eq!(body["background"], "transparent");
    assert_eq!(body["n"], 1);
    assert_eq!(
        body["prompt"],
        "One die-cut sticker logo with a thick white sticker border, flat front view, bold simple emblem style, no text, no letters: A golden honeycomb hexagon with a small bee. Centered, large, transparent background."
    );
    // The rectangle is 32 x 16 pixels. The empty border is cropped away.
    let picture = decode(&png);
    assert_eq!(picture.dimensions(), (32, 16));
    assert!(picture.pixels().all(|p| p.0 == [200, 30, 30, 255]));
}

#[test]
fn clean_up_cuts_alpha_below_128_crops_to_the_box_and_scales_down() {
    // Alpha 127 goes to 0 and does not count for the box. Alpha 128 stays and counts.
    let paint = [
        (0, 0, [9, 9, 9, 127]),      // outside the box, must not widen it
        (20, 10, [10, 20, 30, 128]), // top left of the box
        (59, 49, [40, 50, 60, 255]), // bottom right of the box
        (30, 30, [70, 80, 90, 127]), // inside the box: alpha 0 after the clean-up
        (31, 31, [70, 80, 90, 200]), // inside the box: stays 200
    ];
    let cleaned = decode(&logo::clean_up(&png_with(100, 80, &paint)).unwrap());
    assert_eq!(cleaned.dimensions(), (40, 40), "box (20,10)..(59,49)");
    assert_eq!(cleaned.get_pixel(0, 0).0, [10, 20, 30, 128]);
    assert_eq!(cleaned.get_pixel(39, 39).0, [40, 50, 60, 255]);
    assert_eq!(cleaned.get_pixel(10, 20).0[3], 0, "alpha 127 became 0");
    assert_eq!(cleaned.get_pixel(11, 21).0, [70, 80, 90, 200]);

    // A box bigger than 512 is scaled down with the same shape: 1000 x 500 becomes 512 x 256.
    let big = decode(&logo::clean_up(&png_with_rect(1100, 600, 50, 50, 1049, 549)).unwrap());
    assert_eq!(big.dimensions(), (512, 256));
    // A box that fits keeps its size (no scale up).
    let small = decode(&logo::clean_up(&png_with_rect(1100, 600, 10, 10, 109, 59)).unwrap());
    assert_eq!(small.dimensions(), (100, 50));
}

#[test]
fn clean_up_refuses_a_picture_with_no_visible_pixel_and_a_bad_file() {
    let empty = png_with(10, 10, &[(5, 5, [1, 2, 3, 127])]);
    let error = logo::clean_up(&empty).unwrap_err();
    assert!(error.contains("no visible pixel"), "{error}");
    let error = logo::clean_up(b"not a png").unwrap_err();
    assert!(error.contains("not a PNG"), "{error}");
}

#[test]
fn errors_are_texts_and_never_hold_the_key() {
    let api = FakeApi::start();
    // 401: a fixed text.
    api.reply(
        401,
        format!("{{\"error\":{{\"message\":\"bad key {KEY}\"}}}}"),
    );
    let error = logo::generate(&request(&api, "x")).unwrap_err();
    assert!(error.contains("401"), "{error}");
    assert!(!error.contains(KEY), "{error}");
    // Another status: the message of the API shows, with the key replaced if it is in it.
    api.reply(
        500,
        format!("{{\"error\":{{\"message\":\"the server broke for {KEY}\"}}}}"),
    );
    let error = logo::generate(&request(&api, "x")).unwrap_err();
    assert!(
        error.contains("500") && error.contains("the server broke"),
        "{error}"
    );
    assert!(!error.contains(KEY), "{error}");
    // An answer with no picture.
    api.reply(200, "{\"data\":[]}");
    let error = logo::generate(&request(&api, "x")).unwrap_err();
    assert!(error.contains("b64_json"), "{error}");
    // Bad base64.
    api.reply(200, "{\"data\":[{\"b64_json\":\"@@@\"}]}");
    let error = logo::generate(&request(&api, "x")).unwrap_err();
    assert!(error.contains("base64"), "{error}");
    // No server at that address (nothing listens on port 1).
    let error = logo::generate(&LogoRequest {
        api_base: "http://127.0.0.1:1",
        key: KEY,
        model: "m",
        subject: "x",
        timeout: Duration::from_secs(5),
    })
    .unwrap_err();
    assert!(error.contains("failed"), "{error}");
    assert!(!error.contains(KEY), "{error}");
}

#[test]
fn a_slow_server_ends_at_the_timeout() {
    let api = FakeApi::start();
    api.hold();
    let started = std::time::Instant::now();
    let error = logo::generate(&LogoRequest {
        timeout: Duration::from_millis(300),
        ..request(&api, "x")
    })
    .unwrap_err();
    api.release();
    assert!(error.contains("failed"), "{error}");
    assert!(started.elapsed() < Duration::from_secs(8));
}
