//! Raw-mode terminal handling and ANSI output.
//!
//! The Elixir original had to ask the operating system which terminal it was
//! attached to (`ps -o tty=`) and then drive `stty` at that device by name,
//! because the BEAM hands its children pipes and has no controlling terminal of
//! its own. A native process has none of that trouble: file descriptor 0 *is*
//! the terminal, so raw mode is one `tcsetattr` and the size is one `ioctl`.
//!
//! Both [`RawMode`] and [`AltScreen`] restore what they changed when dropped,
//! including while a panic unwinds — otherwise a crash would leave the shell
//! with no echo.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver};
use std::thread;

const ESC: &str = "\x1b";

/// A bar cursor sits in the gap at the left edge of the cell, so the letter you
/// are about to type stays readable. A block cursor covers it.
const CARET_BAR: &str = "\x1b[5 q";
const CARET_DEFAULT: &str = "\x1b[0 q";

const DEFAULT_SIZE: (usize, usize) = (24, 80);

/// Something arriving from the keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Key(char),
    /// Standard input ended; there will be nothing more.
    Closed,
}

/// Whether this process is attached to a terminal at all.
pub fn is_tty() -> bool {
    // SAFETY: `isatty` only inspects the descriptor and cannot fail in a way
    // that matters here — a non-terminal simply answers 0.
    unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
}

/// The terminal put into raw mode, and restored when this value is dropped.
pub struct RawMode {
    saved: libc::termios,
}

impl RawMode {
    /// Puts the terminal into raw mode so keystrokes arrive one at a time,
    /// unbuffered and unechoed.
    pub fn enable() -> io::Result<RawMode> {
        // SAFETY: both calls take a descriptor we own and a `termios` we own.
        // The struct is fully written by `tcgetattr` before it is read.
        unsafe {
            let mut saved: libc::termios = std::mem::zeroed();

            if libc::tcgetattr(libc::STDIN_FILENO, &mut saved) != 0 {
                return Err(io::Error::last_os_error());
            }

            let mut raw = saved;
            libc::cfmakeraw(&mut raw);

            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(RawMode { saved })
        }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        // SAFETY: restoring the settings this value captured on the way in.
        unsafe {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSAFLUSH, &self.saved);
        }
    }
}

/// The alternate screen buffer, left when this value is dropped.
pub struct AltScreen;

impl AltScreen {
    /// Switches to the alternate screen buffer and hides the cursor.
    pub fn enter() -> AltScreen {
        write(&format!("{ESC}[?1049h{ESC}[?25l{ESC}[2J{CARET_BAR}"));
        AltScreen
    }
}

impl Drop for AltScreen {
    fn drop(&mut self) {
        write(&format!("{ESC}[?25h{CARET_DEFAULT}{ESC}[?1049l{ESC}[0m"));
    }
}

/// Terminal size as `(rows, columns)`, falling back to a sane default.
pub fn size() -> (usize, usize) {
    // SAFETY: `winsize` is plain data that the ioctl fills in; a failed call
    // leaves it zeroed, which the check below rejects.
    unsafe {
        let mut window: libc::winsize = std::mem::zeroed();

        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut window) != 0 {
            return DEFAULT_SIZE;
        }

        if window.ws_row == 0 || window.ws_col == 0 {
            return DEFAULT_SIZE;
        }

        (window.ws_row as usize, window.ws_col as usize)
    }
}

/// Starts a thread that forwards each character from standard input.
///
/// Reading blocks, so it has to live on its own thread: the main loop needs to
/// keep waking on its own timer to advance the clock.
pub fn start_reader() -> Receiver<Input> {
    let (sender, receiver) = mpsc::channel();

    thread::spawn(move || {
        let mut stdin = io::stdin().lock();
        let mut decoder = Decoder::default();
        let mut byte = [0u8; 1];

        loop {
            match stdin.read(&mut byte) {
                Ok(0) | Err(_) => {
                    let _ = sender.send(Input::Closed);
                    return;
                }
                Ok(_) => {
                    if let Some(character) = decoder.push(byte[0]) {
                        if sender.send(Input::Key(character)).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    });

    receiver
}

/// Paints a frame.
///
/// The frame is wrapped in synchronized-output markers so terminals that
/// support them present it in one piece instead of tearing mid-repaint;
/// terminals that don't simply ignore the sequence.
pub fn paint(frame: &str, caret: Option<(usize, usize)>) {
    let mut out = String::with_capacity(frame.len() + 64);

    out.push_str(ESC);
    out.push_str("[?2026h");
    out.push_str(ESC);
    out.push_str("[H");
    out.push_str(ESC);
    out.push_str("[2J");
    out.push_str(frame);

    match caret {
        Some((row, column)) => {
            out.push_str(&format!("{ESC}[{row};{column}H{ESC}[?25h"));
        }
        None => out.push_str(&format!("{ESC}[?25l")),
    }

    out.push_str(ESC);
    out.push_str("[?2026l");

    write(&out);
}

fn write(text: &str) {
    let mut out = io::stdout().lock();
    let _ = out.write_all(text.as_bytes());
    let _ = out.flush();
}

/// Reassembles UTF-8 characters from a stream of single bytes.
///
/// Keystrokes arrive one byte at a time, so a multi-byte character shows up in
/// pieces; anything that is not valid UTF-8 is dropped rather than guessed at.
#[derive(Default)]
struct Decoder {
    pending: Vec<u8>,
}

impl Decoder {
    fn push(&mut self, byte: u8) -> Option<char> {
        self.pending.push(byte);

        match std::str::from_utf8(&self.pending) {
            Ok(text) => {
                let character = text.chars().next();
                self.pending.clear();
                character
            }
            // Still waiting for continuation bytes, unless the sequence has
            // already run past the longest a character can be.
            Err(error) if error.error_len().is_none() && self.pending.len() < 4 => None,
            Err(_) => {
                self.pending.clear();
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_arrives_one_character_at_a_time() {
        let mut decoder = Decoder::default();

        assert_eq!(decoder.push(b'a'), Some('a'));
        assert_eq!(decoder.push(b'b'), Some('b'));
    }

    #[test]
    fn a_multi_byte_character_is_reassembled_from_its_pieces() {
        let mut decoder = Decoder::default();
        let bytes = "é".as_bytes();

        assert_eq!(bytes.len(), 2);
        assert_eq!(decoder.push(bytes[0]), None);
        assert_eq!(decoder.push(bytes[1]), Some('é'));
    }

    #[test]
    fn a_four_byte_character_is_reassembled_too() {
        let mut decoder = Decoder::default();
        let bytes = "🦀".as_bytes();

        assert_eq!(bytes.len(), 4);
        assert_eq!(decoder.push(bytes[0]), None);
        assert_eq!(decoder.push(bytes[1]), None);
        assert_eq!(decoder.push(bytes[2]), None);
        assert_eq!(decoder.push(bytes[3]), Some('🦀'));
    }

    #[test]
    fn invalid_bytes_are_dropped_rather_than_guessed_at() {
        let mut decoder = Decoder::default();

        assert_eq!(decoder.push(0xFF), None);
        // The decoder recovers: the next good byte still reads as itself.
        assert_eq!(decoder.push(b'a'), Some('a'));
    }

    #[test]
    fn a_truncated_sequence_does_not_swallow_the_stream() {
        let mut decoder = Decoder::default();

        // The lead byte of a four-byte character, then nonsense.
        decoder.push(0xF0);
        decoder.push(0x9F);
        decoder.push(0x9F);
        decoder.push(0x9F);

        assert_eq!(decoder.push(b'x'), Some('x'));
    }

    #[test]
    fn escape_and_control_bytes_pass_straight_through() {
        let mut decoder = Decoder::default();

        assert_eq!(decoder.push(0x1b), Some('\x1b'));
        assert_eq!(decoder.push(0x7f), Some('\x7f'));
        assert_eq!(decoder.push(b'\t'), Some('\t'));
    }

    #[test]
    fn the_size_is_always_usable_even_with_no_terminal() {
        let (rows, columns) = size();

        assert!(rows > 0 && columns > 0);
    }
}
