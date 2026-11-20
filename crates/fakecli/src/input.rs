//! Input of `fake-cli`: bytes to keys. It knows bracketed paste (`ESC[200~ ... ESC[201~`).

use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

const PASTE_END: &[u8] = b"\x1b[201~";

/// How long a lone ESC waits for the rest of a sequence.
const ESC_WAIT: Duration = Duration::from_millis(50);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Text between the paste marks.
    Paste(String),
    Enter,
    Esc,
    Backspace,
    CtrlC,
    Char(char),
    /// Any other escape sequence (arrows and so on).
    Seq(Vec<u8>),
}

#[derive(Debug)]
enum State {
    Normal,
    Esc,
    Csi(Vec<u8>),
    Ss3,
    Utf8(Vec<u8>, usize),
    Paste(Vec<u8>),
}

/// Byte-by-byte parser. It keeps its state between chunks.
#[derive(Debug)]
pub struct Parser {
    state: State,
}

impl Parser {
    pub fn new() -> Parser {
        Parser {
            state: State::Normal,
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Key> {
        let mut keys = Vec::new();
        for &byte in bytes {
            self.byte(byte, &mut keys);
        }
        keys
    }

    pub fn has_pending_esc(&self) -> bool {
        matches!(self.state, State::Esc)
    }

    /// A lone ESC that nothing followed is the Esc key.
    pub fn flush_esc(&mut self) -> Option<Key> {
        if self.has_pending_esc() {
            self.state = State::Normal;
            Some(Key::Esc)
        } else {
            None
        }
    }

    fn byte(&mut self, byte: u8, keys: &mut Vec<Key>) {
        match std::mem::replace(&mut self.state, State::Normal) {
            State::Normal => self.normal(byte, keys),
            State::Esc => match byte {
                b'[' => self.state = State::Csi(Vec::new()),
                b'O' => self.state = State::Ss3,
                _ => {
                    keys.push(Key::Esc);
                    self.normal(byte, keys);
                }
            },
            State::Ss3 => keys.push(Key::Seq(vec![0x1b, b'O', byte])),
            State::Csi(mut seq) => {
                seq.push(byte);
                if (0x40..=0x7e).contains(&byte) {
                    if seq == b"200~" {
                        self.state = State::Paste(Vec::new());
                    } else {
                        let mut full = vec![0x1b, b'['];
                        full.extend(seq);
                        keys.push(Key::Seq(full));
                    }
                } else {
                    self.state = State::Csi(seq);
                }
            }
            State::Utf8(mut seq, need) => {
                seq.push(byte);
                if seq.len() < need {
                    self.state = State::Utf8(seq, need);
                } else {
                    keys.extend(String::from_utf8_lossy(&seq).chars().map(Key::Char));
                }
            }
            State::Paste(mut data) => {
                data.push(byte);
                if data.ends_with(PASTE_END) {
                    data.truncate(data.len() - PASTE_END.len());
                    keys.push(Key::Paste(String::from_utf8_lossy(&data).into_owned()));
                } else {
                    self.state = State::Paste(data);
                }
            }
        }
    }

    fn normal(&mut self, byte: u8, keys: &mut Vec<Key>) {
        match byte {
            0x1b => self.state = State::Esc,
            b'\r' => keys.push(Key::Enter),
            0x7f | 0x08 => keys.push(Key::Backspace),
            0x03 => keys.push(Key::CtrlC),
            0xc0..=0xdf => self.state = State::Utf8(vec![byte], 2),
            0xe0..=0xef => self.state = State::Utf8(vec![byte], 3),
            0xf0..=0xf7 => self.state = State::Utf8(vec![byte], 4),
            _ => keys.push(Key::Char(char::from(byte))),
        }
    }
}

/// What `Input::next` gives.
#[derive(Debug, PartialEq, Eq)]
pub enum Next {
    Key(Key),
    Timeout,
    /// Stdin is closed.
    Eof,
}

/// Keys from the channel of raw chunks that the reader thread sends.
pub struct Input {
    chunks: Receiver<Vec<u8>>,
    parser: Parser,
    queue: std::collections::VecDeque<Key>,
}

impl Input {
    pub fn new(chunks: Receiver<Vec<u8>>) -> Input {
        Input {
            chunks,
            parser: Parser::new(),
            queue: Default::default(),
        }
    }

    /// The next key. `deadline` is `None` for no limit.
    pub fn next(&mut self, deadline: Option<Instant>) -> Next {
        loop {
            if let Some(key) = self.queue.pop_front() {
                return Next::Key(key);
            }
            let now = Instant::now();
            let mut wait = match deadline {
                Some(deadline) if deadline <= now => return Next::Timeout,
                Some(deadline) => deadline - now,
                None => Duration::from_secs(3600),
            };
            if self.parser.has_pending_esc() {
                wait = wait.min(ESC_WAIT);
            }
            match self.chunks.recv_timeout(wait) {
                Ok(chunk) => self.queue.extend(self.parser.feed(&chunk)),
                Err(RecvTimeoutError::Timeout) => {
                    if let Some(key) = self.parser.flush_esc() {
                        return Next::Key(key);
                    }
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return match self.parser.flush_esc() {
                        Some(key) => Next::Key(key),
                        None => Next::Eof,
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(chunks: &[&[u8]]) -> Vec<Key> {
        let mut parser = Parser::new();
        chunks.iter().flat_map(|chunk| parser.feed(chunk)).collect()
    }

    #[test]
    fn paste_then_enter() {
        assert_eq!(
            keys(&[b"\x1b[200~line one\nline two\x1b[201~\r"]),
            vec![Key::Paste("line one\nline two".into()), Key::Enter]
        );
    }

    #[test]
    fn paste_split_in_chunks() {
        assert_eq!(
            keys(&[b"\x1b[20", b"0~ab\x1b", b"[201", b"~", b"\r"]),
            vec![Key::Paste("ab".into()), Key::Enter]
        );
    }

    #[test]
    fn paste_keeps_cr_and_lone_esc_inside() {
        assert_eq!(
            keys(&[b"\x1b[200~a\r\x1bb\x1b[201~"]),
            vec![Key::Paste("a\r\x1bb".into())]
        );
    }

    #[test]
    fn plain_keys_and_sequences() {
        assert_eq!(
            keys(&[b"a1\r\x7f\x03\x1b[A\x1bOP\xc3\xa9"]),
            vec![
                Key::Char('a'),
                Key::Char('1'),
                Key::Enter,
                Key::Backspace,
                Key::CtrlC,
                Key::Seq(b"\x1b[A".to_vec()),
                Key::Seq(b"\x1bOP".to_vec()),
                Key::Char('é'),
            ]
        );
    }

    #[test]
    fn lone_esc_waits_then_flushes() {
        let mut parser = Parser::new();
        assert!(parser.feed(b"\x1b").is_empty());
        assert!(parser.has_pending_esc());
        assert_eq!(parser.flush_esc(), Some(Key::Esc));
        assert_eq!(parser.flush_esc(), None);
        // ESC followed by a normal byte: Esc, then the byte.
        assert_eq!(parser.feed(b"\x1bx"), vec![Key::Esc, Key::Char('x')]);
    }

    #[test]
    fn input_gives_a_lone_esc_after_a_short_wait() {
        let (sender, receiver) = std::sync::mpsc::channel();
        let mut input = Input::new(receiver);
        sender.send(b"\x1b".to_vec()).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        assert_eq!(input.next(Some(deadline)), Next::Key(Key::Esc));
        assert_eq!(
            input.next(Some(Instant::now() + Duration::from_millis(20))),
            Next::Timeout
        );
        drop(sender);
        assert_eq!(input.next(None), Next::Eof);
    }
}
