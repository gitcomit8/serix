/*
 * io.rs - Line-buffered stdin reader for userspace
 *
 * Provides read_line() which reads one newline-terminated line from
 * STDIN with character echo and backspace support.
 */

extern crate alloc;
use alloc::string::String;
use crate::{STDIN, STDOUT, read, write as sys_write};

/*
 * read_line - Read a line from stdin with echo
 *
 * Reads bytes one at a time from STDIN, echoing each printable byte back.
 * Handles backspace (0x08, 0x7F) by removing the last character.
 * Returns on CR or LF. Returns None if stdin yields zero bytes (EOF).
 */
pub fn read_line() -> Option<String> {
	let mut line = String::new();
	let mut buf = [0u8; 1];
	loop {
		let n = read(STDIN, &mut buf);
		if n == 0 {
			return None;
		}
		match buf[0] {
			b'\r' | b'\n' => {
				sys_write(STDOUT, b"\n");
				return Some(line);
			}
			0x08 | 0x7F => {
				if !line.is_empty() {
					line.pop();
					sys_write(STDOUT, b"\x08 \x08");
				}
			}
			c => {
				if let Ok(ch) = core::str::from_utf8(&[c]) {
					line.push_str(ch);
				}
				sys_write(STDOUT, &buf[..1]);
			}
		}
	}
}
