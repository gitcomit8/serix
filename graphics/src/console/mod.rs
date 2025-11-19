/*
 * Framebuffer Console
 *
 * Implements a text console using a framebuffer and bitmap font.
 * Provides scrolling, character rendering, and format macro support.
 */

use core::fmt;
use core::fmt::Write;
use core::ptr::write_volatile;

#[cfg(feature = "global-console")]
use spin::{Mutex, MutexGuard};

/*
 * 8x16 bitmap font for ASCII characters 32-127
 * Each character is 16 bytes (16 rows of 8 pixels each)
 */
const FONT_8X16: &[u8] = include_bytes!("font8x16.bin");

/* Global console instance */
#[cfg(feature = "global-console")]
static GLOBAL_CONSOLE: Mutex<Option<FramebufferConsole>> = Mutex::new(None);

/*
 * struct FramebufferConsole - Text console using framebuffer
 * @framebuffer: Pointer to framebuffer memory
 * @width: Width in pixels
 * @height: Height in pixels
 * @pitch: Bytes per scanline
 * @cursor_x: Current cursor column (in characters)
 * @cursor_y: Current cursor row (in characters)
 */
pub struct FramebufferConsole {
	framebuffer: *mut u8,
	width: usize,
	height: usize,
	pitch: usize,
	cursor_x: usize,
	cursor_y: usize,
}

unsafe impl Sync for FramebufferConsole {}
unsafe impl Send for FramebufferConsole {}

impl FramebufferConsole {
	/*
	 * new - Create a new framebuffer console
	 * @framebuffer: Pointer to framebuffer memory
	 * @width: Width in pixels
	 * @height: Height in pixels
	 * @pitch: Bytes per scanline
	 */
	pub unsafe fn new(framebuffer: *mut u8, width: usize, height: usize, pitch: usize) -> Self {
		Self {
			framebuffer,
			width,
			height,
			pitch,
			cursor_x: 0,
			cursor_y: 0,
		}
	}

	/*
	 * put_char - Output a character at the current cursor position
	 * @c: Character to output
	 *
	 * Handles newlines, carriage returns, and automatic line wrapping.
	 */
	fn put_char(&mut self, c: char) {
		if c == '\n' {
			self.cursor_x = 0;
			self.cursor_y += 1;
			self.scroll_if_needed();
			return;
		}
		if c == '\r' {
			self.cursor_x = 0;
			return;
		}
		self.draw_char(c, self.cursor_x, self.cursor_y);

		self.cursor_x += 1;
		if self.cursor_x * 8 >= self.width {
			self.cursor_x = 0;
			self.cursor_y += 1;
			self.scroll_if_needed();
		}
	}

	/*
	 * scroll_if_needed - Scroll the display if cursor is off-screen
	 */
	fn scroll_if_needed(&mut self) {
		let max_lines = self.height / 16;
		if self.cursor_y >= max_lines {
			self.scroll_up();
			self.cursor_y = max_lines - 1;
		}
	}

	/*
	 * scroll_up - Scroll framebuffer contents up by one line
	 *
	 * Moves all scanlines up by 16 pixels (one character height) and clears the bottom line.
	 */
	fn scroll_up(&mut self) {
		unsafe {
			let fb = self.framebuffer;
			let pitch = self.pitch;
			let height_bytes = self.height * pitch;

			//Move framebuffer lines up by one character height (16 pixels)
			let src = fb.add(16 * pitch);
			core::ptr::copy(src, fb, height_bytes - 16 * pitch);

			//Clear the last character line by writing zero
			let clear_start = fb.add(height_bytes - 16 * pitch);
			for i in 0..(16 * pitch) {
				write_volatile(clear_start.add(i), 0);
			}
		}
	}

	fn draw_char(&mut self, c: char, x_char: usize, y_char: usize) {
		let c = c as u8;
		let glyph = if c < 32 || c > 127 {
			&FONT_8X16[(b'?' - 32) as usize * 16..][..16]
		} else {
			&FONT_8X16[(c - 32) as usize * 16..][..16]
		};

		let fb = self.framebuffer;
		let pitch = self.pitch;
		let x_pixel = x_char * 8;
		let y_pixel = y_char * 16;

		unsafe {
			for (row, &bits) in glyph.iter().enumerate() {
				for bit in 0..8 {
					let pixel_on = (bits & (1 << (7 - bit))) != 0;
					let pixel = if pixel_on { [0xFF, 0xFF, 0xFF, 0x00] } else { [0x00, 0x00, 0x00, 0x00] };
					let offset = (y_pixel + row) * pitch + (x_pixel + bit) * 4;
					for p in 0..4 {
						write_volatile(fb.add(offset + p), pixel[p]);
					}
				}
			}
		}
	}

	fn write_string(&mut self, s: &str) {
		for c in s.chars() {
			self.put_char(c);
		}
	}
}

impl Write for FramebufferConsole {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		self.write_string(s);
		Ok(())
	}
}

#[cfg(feature = "global-console")]
pub fn init_console(framebuffer: *mut u8, width: usize, height: usize, pitch: usize) {
	let mut con = GLOBAL_CONSOLE.lock();
	*con = Some(unsafe { FramebufferConsole::new(framebuffer, width, height, pitch) });
}

#[cfg(feature = "global-console")]
pub fn console() -> impl Write + 'static {
	struct ConsoleGuard<'a> {
		guard: MutexGuard<'a, Option<FramebufferConsole>>,
	}

	impl<'a> Write for ConsoleGuard<'a> {
		fn write_str(&mut self, s: &str) -> fmt::Result {
			if let Some(console) = &mut *self.guard {
				console.write_string(s);
				Ok(())
			} else {
				Err(fmt::Error)
			}
		}
	}
	ConsoleGuard {
		guard: GLOBAL_CONSOLE.lock(),
	}
}

#[macro_export]
macro_rules! fb_print {
	($($arg:tt)*) => {{
		use core::fmt::Write as _;
		let _ = $crate::console::console().write_fmt(format_args!($($arg)*));
	}};
}

#[macro_export]
macro_rules! fb_println {
	() => {
		$crate::fb_print!("\n")
	};
	($fmt:expr) => {
		$crate::fb_print!(concat!($fmt, "\n"))
	};
	($fmt:expr, $($arg:tt)*) => {
		$crate::fb_print!(concat!($fmt, "\n"), $($arg)*)
	};
}