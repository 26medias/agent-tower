//! A fake image API on a local port, for the tests of the logo. It uses `std` only. No test
//! calls the real API: the base URL of the Tower or of `logo::generate` is `FakeApi::base`.
#![allow(dead_code)]

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::{ImageFormat, Rgba, RgbaImage};
use std::collections::VecDeque;
use std::io::{Cursor, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::thread;

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// One request that the server got.
#[derive(Debug, Clone)]
pub struct Recorded {
    /// For example `POST /v1/images/generations HTTP/1.1`.
    pub request_line: String,
    /// Header names in lower case.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Recorded {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }
}

struct Shared {
    requests: Mutex<Vec<Recorded>>,
    replies: Mutex<VecDeque<(u16, String)>>,
    /// `true`: the server answers at once. `false`: it holds each answer until `release`.
    open: (Mutex<bool>, Condvar),
}

pub struct FakeApi {
    /// For example `http://127.0.0.1:41234`.
    pub base: String,
    shared: Arc<Shared>,
}

impl FakeApi {
    pub fn start() -> FakeApi {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a local port");
        let base = format!("http://{}", listener.local_addr().unwrap());
        let shared = Arc::new(Shared {
            requests: Mutex::new(Vec::new()),
            replies: Mutex::new(VecDeque::new()),
            open: (Mutex::new(true), Condvar::new()),
        });
        let for_thread = Arc::clone(&shared);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = Arc::clone(&for_thread);
                thread::spawn(move || serve(stream, &shared));
            }
        });
        FakeApi { base, shared }
    }

    /// The next answer. An answer is used once, in the order of the calls. With no answer left,
    /// the server says 404.
    pub fn reply(&self, status: u16, body: impl Into<String>) {
        lock(&self.shared.replies).push_back((status, body.into()));
    }

    /// Answer 200 with a picture, in the form of the real API.
    pub fn reply_picture(&self, png: &[u8]) {
        self.reply(
            200,
            format!(
                "{{\"data\":[{{\"b64_json\":\"{}\"}}]}}",
                STANDARD.encode(png)
            ),
        );
    }

    /// Hold the answers until `release`.
    pub fn hold(&self) {
        *lock(&self.shared.open.0) = false;
    }

    pub fn release(&self) {
        *lock(&self.shared.open.0) = true;
        self.shared.open.1.notify_all();
    }

    pub fn requests(&self) -> Vec<Recorded> {
        lock(&self.shared.requests).clone()
    }
}

fn serve(mut stream: TcpStream, shared: &Shared) {
    let mut data = Vec::new();
    let mut chunk = [0u8; 4096];
    let head_end = loop {
        if let Some(position) = data.windows(4).position(|w| w == b"\r\n\r\n") {
            break position + 4;
        }
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    };
    let head = String::from_utf8_lossy(&data[..head_end]).into_owned();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default().to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(n, v)| (n.trim().to_ascii_lowercase(), v.trim().to_owned()))
        .collect();
    let length: usize = headers
        .iter()
        .find(|(n, _)| n == "content-length")
        .and_then(|(_, v)| v.parse().ok())
        .unwrap_or(0);
    while data.len() < head_end + length {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => data.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&data[head_end..]).into_owned();
    lock(&shared.requests).push(Recorded {
        request_line,
        headers,
        body,
    });
    {
        let mut open = lock(&shared.open.0);
        while !*open {
            open = shared
                .open
                .1
                .wait(open)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    let (status, body) = lock(&shared.replies)
        .pop_front()
        .unwrap_or((404, "{}".to_owned()));
    let head = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
}

/// A PNG file: `width` x `height`, transparent, with the pixels of `paint` set (`x`, `y`, color).
pub fn png_with(width: u32, height: u32, paint: &[(u32, u32, [u8; 4])]) -> Vec<u8> {
    let mut picture = RgbaImage::from_pixel(width, height, Rgba([0, 0, 0, 0]));
    for &(x, y, color) in paint {
        picture.put_pixel(x, y, Rgba(color));
    }
    let mut out = Cursor::new(Vec::new());
    picture.write_to(&mut out, ImageFormat::Png).unwrap();
    out.into_inner()
}

/// A PNG file with an opaque red rectangle from (`x0`, `y0`) to (`x1`, `y1`), both included.
pub fn png_with_rect(width: u32, height: u32, x0: u32, y0: u32, x1: u32, y1: u32) -> Vec<u8> {
    let mut paint = Vec::new();
    for y in y0..=y1 {
        for x in x0..=x1 {
            paint.push((x, y, [200, 30, 30, 255]));
        }
    }
    png_with(width, height, &paint)
}
