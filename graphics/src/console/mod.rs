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
static FONT_8X16: &[u8] = include_bytes!("font8x16.bin");
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 16;

/*
 * Global console instance
 */
#[cfg(feature = "global-console")]
static GLOBAL_CONSOLE: Mutex<Option<FramebufferConsole>> = Mutex::new(None);

/*
 * struct Tty - The terminal state
 * @fb_base: Pointer to framebuffer memory
 * @fb_width: Framebuffer width in pixels
 * @fb_height: Framebuffer height in pixels
 * @fb_pitch: Bytes per scanline
 * @cols: Terminal width in characters
 * @rows: Terminal height in characters
 * @x: Current cursor column (in characters)
 * @y: Current cursor row (in characters)
 * @color_fg: Foreground color (0x00RRGGBB)
 * @color_bg: Background color (0x00RRGGBB)
 *
 * Owns framebuffer access and terminal cursor state.
 */
pub struct Tty {
	fb_base: *mut u8,
	fb_width: usize,
	fb_height: usize,
	fb_pitch: usize,

	// Terminal dimensions in characters
	cols: usize,
	rows: usize,

	// Cursor position in characters
	x: usize,
	y: usize,

	// Colors (0x00RRGGBB format)
	color_fg: u32,
	color_bg: u32,
}

unsafe impl Send for Tty {}
unsafe impl Sync for Tty {}

pub static KERNEL_TTY: Mutex<Option<Tty>> = Mutex::new(None);

impl Tty {
	/*
	 * clear - Clear the entire framebuffer
	 *
	 * Fills the framebuffer with zeros and resets cursor to (0, 0).
	 */
	pub fn clear(&mut self) {
		let total_bytes = self.fb_pitch * self.fb_height;
		unsafe {
			core::ptr::write_bytes(self.fb_base, 0, total_bytes);
		}
		self.x = 0;
		self.y = 0;
	}

	/*
	 * scroll - Scroll the framebuffer up by one character line
	 *
	 * Moves all scanlines up by FONT_HEIGHT pixels and clears
	 * the bottom line.
	 */
	fn scroll(&mut self) {
		let line_size = self.fb_pitch * FONT_HEIGHT;
		let total_size = line_size * self.rows;

		unsafe {
			// Memmove: Copy lines 1..N upto 0..N-1
			core::ptr::copy(
				self.fb_base.add(line_size),
				self.fb_base,
				total_size - line_size,
			);
			// Zero out the new last line
			core::ptr::write_bytes(self.fb_base.add(total_size - line_size), 0, line_size);
		}
	}

	/*
	 * new_line - Advance to next line
	 *
	 * Moves cursor to start of next line, scrolling if necessary.
	 */
	fn new_line(&mut self) {
		self.x = 0;
		self.y += 1;
		if self.y >= self.rows {
			self.scroll();
			self.y = self.rows - 1;
		}
	}

	/*
	 * write_char - Write a single character
	 * @c: Character to write
	 *
	 * Handles newlines, backspace, and automatic line wrapping.
	 */
	pub fn write_char(&mut self, c: char) {
		match c {
			'\n' => self.new_line(),
			'\x08' => {
				if self.x > 0 {
					self.x -= 1;
					self.draw_char(self.x, self.y, ' ');
				}
			}
			_ => {
				if self.x >= self.cols {
					self.new_line();
				}
				self.draw_char(self.x, self.y, c);
				self.x += 1;
			}
		}
	}

	/*
	 * draw_char - Draw a character at specified position
	 * @cx: Character column
	 * @cy: Character row
	 * @c: Character to draw
	 *
	 * Renders the character using the 8x16 bitmap font.
	 */
	fn draw_char(&mut self, cx: usize, cy: usize, c: char) {
		let glyph_index = match c {
			' '..='~' => c as usize,
			_ => 0xFE,
		};

		let screen_x = cx * FONT_WIDTH;
		let screen_y = cy * FONT_HEIGHT;
		let glyph = &FONT_8X16[glyph_index * 16..(glyph_index + 1) * 16];

		for row in 0..FONT_HEIGHT {
			let bits = glyph[row];
			for col in 0..FONT_WIDTH {
				let offset = (screen_y + row) * self.fb_pitch + (screen_x + col) * 4;
				let color = if (bits & (1 << (7 - col))) != 0 {
					self.color_fg
				} else {
					self.color_bg
				};
				unsafe {
					(self.fb_base.add(offset) as *mut u32).write_volatile(color);
				}
			}
		}
	}
}

impl fmt::Write for Tty {
	fn write_str(&mut self, s: &str) -> fmt::Result {
		for c in s.chars() {
			self.write_char(c);
		}
		Ok(())
	}
}

/*
 * _print - Internal print function for macros
 * @args: Format arguments
 *
 * Called by fb_print! and fb_println! macros.
 */
#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
	use x86_64::instructions::interrupts;

	interrupts::without_interrupts(|| {
		if let Some(tty) = KERNEL_TTY.lock().as_mut() {
			let _ = fmt::Write::write_fmt(&mut *tty, args);
		}
	})
}

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
					let pixel = if pixel_on {
						[0xFF, 0xFF, 0xFF, 0x00]
					} else {
						[0x00, 0x00, 0x00, 0x00]
					};
					let offset = (y_pixel + row) * pitch + (x_pixel + bit) * 4;
					for p in 0..4 {
						write_volatile(fb.add(offset + p), pixel[p]);
					}
				}
			}
		}
	}

	/*
	 * write_string - Write a string of characters
	 * @s: String to write
	 */
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

// #[cfg(feature = "global-console")]
// pub fn init_console(framebuffer: *mut u8, width: usize, height: usize, pitch: usize) {
// 	let mut con = GLOBAL_CONSOLE.lock();
// 	*con = Some(unsafe { FramebufferConsole::new(framebuffer, width, height, pitch) });
// }

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

/*
 * init_console - Initialize the kernel TTY
 * @base: Framebuffer base pointer
 * @width: Width in pixels
 * @height: Height in pixels
 * @pitch: Bytes per scanline
 *
 * Creates and initializes the global kernel TTY.
 */
pub unsafe fn init_console(base: *mut u8, width: usize, height: usize, pitch: usize) {
	let cols = width / FONT_WIDTH;
	let rows = height / FONT_HEIGHT;

	let mut tty = Tty {
		fb_base: base,
		fb_width: width,
		fb_pitch: pitch,
		fb_height: height,
		cols,
		rows,
		x: 0,
		y: 0,
		color_fg: 0x00AAAAAA,
		color_bg: 0x00000000,
	};
	tty.clear();
	*KERNEL_TTY.lock() = Some(tty);
}

/*
 * fb_print! - Print formatted text to framebuffer
 *
 * Usage: fb_print!("format string {}", args...)
 */
#[macro_export]
macro_rules! fb_print {
	($($arg:tt)*) => {{
		use core::fmt::Write as _;
		let _ = $crate::console::console().write_fmt(format_args!($($arg)*));
	}};
}

/*
 * fb_println! - Print formatted text with newline to framebuffer
 *
 * Usage: fb_println!("format string {}", args...)
 */
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
