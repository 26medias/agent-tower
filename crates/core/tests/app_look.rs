//! The tools of the MCP server for the look of a floor (`docs/ipc-final.md`): `floor_look`,
//! `floor_logo_set` and `floor_look_set`. No test calls an image API.

mod app_support;
mod fake_api;

use app_support::Rig;
use fake_api::png_with_rect;
use serde_json::{Value, json};
use std::fs;

fn rig() -> Rig {
    let rig = Rig::new();
    rig.create_floor();
    rig.start("ceo");
    rig
}

fn look(rig: &Rig) -> Value {
    let answer = rig.tool("ceo", "floor_look", json!({}));
    assert!(!answer.is_error, "{}", answer.text);
    serde_json::from_str(&answer.text).unwrap()
}

fn logo_size(rig: &Rig) -> (u32, u32) {
    let bytes = rig.tower.floor_logo(&rig.cwd).unwrap().expect("a logo");
    let picture = image::load_from_memory(&bytes).unwrap();
    (picture.width(), picture.height())
}

#[test]
fn an_employee_sets_the_logo_from_a_picture_of_the_project() {
    let rig = rig();
    assert_eq!(look(&rig)["has_logo"], json!(false));
    let before = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(before.logo_version, None);

    // A picture of 1200 x 900 with a visible box of 1000 x 500 and an empty border.
    fs::create_dir(rig.cwd.join("art")).unwrap();
    fs::write(
        rig.cwd.join("art/logo.png"),
        png_with_rect(1200, 900, 100, 200, 1099, 699),
    )
    .unwrap();
    let answer = rig.tool("ceo", "floor_logo_set", json!({"file": "art/logo.png"}));
    assert!(!answer.is_error, "{}", answer.text);

    // The border is cropped and the picture fits 512 x 512 with its proportions.
    assert_eq!(logo_size(&rig), (512, 256));
    let after = rig.tower.floor_open(&rig.cwd).unwrap();
    assert!(after.logo_version.is_some());
    assert_eq!(look(&rig)["has_logo"], json!(true));

    // An absolute path works too, and a small picture keeps its size.
    let small = rig.cwd.join("small.png");
    fs::write(&small, png_with_rect(40, 40, 0, 0, 39, 19)).unwrap();
    let answer = rig.tool("ceo", "floor_logo_set", json!({"file": small}));
    assert!(!answer.is_error, "{}", answer.text);
    assert_eq!(logo_size(&rig), (40, 20));
}

#[test]
fn a_jpeg_picture_becomes_a_png_logo() {
    let rig = rig();
    let picture = image::RgbImage::from_pixel(64, 32, image::Rgb([10, 120, 200]));
    picture.save(rig.cwd.join("brand.jpg")).unwrap();
    let answer = rig.tool("ceo", "floor_logo_set", json!({"file": "brand.jpg"}));
    assert!(!answer.is_error, "{}", answer.text);
    let bytes = rig.tower.floor_logo(&rig.cwd).unwrap().unwrap();
    assert_eq!(
        image::guess_format(&bytes).unwrap(),
        image::ImageFormat::Png
    );
    assert_eq!(logo_size(&rig), (64, 32));
}

#[test]
fn a_bad_file_is_refused_and_the_logo_stays() {
    let rig = rig();
    fs::write(
        rig.cwd.join("good.png"),
        png_with_rect(30, 30, 0, 0, 29, 29),
    )
    .unwrap();
    let answer = rig.tool("ceo", "floor_logo_set", json!({"file": "good.png"}));
    assert!(!answer.is_error, "{}", answer.text);
    let kept = rig.tower.floor_logo(&rig.cwd).unwrap().unwrap();

    // A file outside the project folder, also through a link inside the project.
    let outside = rig.root.path().join("outside.png");
    fs::write(&outside, png_with_rect(20, 20, 0, 0, 19, 19)).unwrap();
    std::os::unix::fs::symlink(&outside, rig.cwd.join("link.png")).unwrap();
    fs::write(rig.cwd.join("notes.txt"), "not a picture").unwrap();
    fs::write(
        rig.cwd.join("mark.svg"),
        "<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
    )
    .unwrap();
    fs::write(rig.cwd.join("empty.png"), png_with_rect(20, 20, 1, 1, 0, 0)).unwrap();
    let outside_text = outside.to_str().unwrap().to_owned();
    for (file, expected) in [
        (outside_text.as_str(), "must be inside the project folder"),
        ("link.png", "must be inside the project folder"),
        ("../outside.png", "must be inside the project folder"),
        ("missing.png", "Cannot find the file"),
        ("notes.txt", "must be a PNG, a JPEG or a WebP"),
        ("mark.svg", "Convert an SVG file"),
        ("empty.png", "no visible pixel"),
        (".", "is not a file"),
        ("", "is empty"),
    ] {
        let answer = rig.tool("ceo", "floor_logo_set", json!({"file": file}));
        assert!(answer.is_error, "{file}: {}", answer.text);
        assert!(answer.text.contains(expected), "{file}: {}", answer.text);
        assert_eq!(
            rig.tower.floor_logo(&rig.cwd).unwrap().unwrap(),
            kept,
            "{file}"
        );
    }
    let answer = rig.tool("ceo", "floor_logo_set", json!({}));
    assert!(
        answer.is_error && answer.text.contains("\"file\" is missing"),
        "{}",
        answer.text
    );
}

#[test]
fn an_employee_changes_the_name_and_the_description_and_the_rest_of_the_file_stays() {
    let rig = rig();
    let file = rig.cwd.join(".swarm/floor.toml");
    let mut text = fs::read_to_string(&file).unwrap();
    text.push_str("future_key = 7\n");
    // A top-level key must be before the first table of the file.
    let text = format!(
        "future_key = 7\n{}",
        text.trim_end_matches("future_key = 7\n")
    );
    fs::write(&file, text).unwrap();

    let answer = rig.tool(
        "ceo",
        "floor_look_set",
        json!({"name": "  Candle Shop ", "description": "A shop for candles."}),
    );
    assert!(!answer.is_error, "{}", answer.text);
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(view.name, "Candle Shop");
    assert_eq!(view.description.as_deref(), Some("A shop for candles."));
    assert_eq!(look(&rig)["name"], json!("Candle Shop"));
    let list = rig.tower.floor_list().unwrap();
    assert_eq!(list[0].name.as_deref(), Some("Candle Shop"));
    assert!(
        fs::read_to_string(&file)
            .unwrap()
            .contains("future_key = 7")
    );

    // Only the description: the name stays. An empty description removes it.
    let answer = rig.tool("ceo", "floor_look_set", json!({"description": ""}));
    assert!(!answer.is_error, "{}", answer.text);
    let view = rig.tower.floor_open(&rig.cwd).unwrap();
    assert_eq!(
        (view.name.as_str(), view.description),
        ("Candle Shop", None)
    );

    for (args, expected) in [
        (json!({}), "Give the argument"),
        (json!({"name": "   "}), "The name is empty"),
        (json!({"name": "a\nb"}), "one line"),
        (json!({"name": "x".repeat(61)}), "too long"),
        (json!({"description": "x".repeat(301)}), "too long"),
        (json!({"name": 5}), "must be a text"),
    ] {
        let answer = rig.tool("ceo", "floor_look_set", args.clone());
        assert!(answer.is_error, "{args}: {}", answer.text);
        assert!(answer.text.contains(expected), "{args}: {}", answer.text);
    }
    assert_eq!(rig.tower.floor_open(&rig.cwd).unwrap().name, "Candle Shop");
}
