/*
 * Framebuffer Console
 *
 * Implements a text console (Tty) on top of the Limine framebuffer and
 * provides the kprint!/kprintln! format macros.
 *
 * There is exactly one framebuffer console: KERNEL_TTY. Every access to it
 * disables interrupts and takes the spin lock, so the console is safe to use
 * from interrupt context without deadlocking against a preempted printer.
 */

use core::fmt;

use spin::Mutex;

/*
 * 8x16 bitmap font for ASCII characters 32-127
 * Each character is 16 bytes (16 rows of 8 pixels each)
 */
static FONT_8X16: &[u8] = include_bytes!("font8x16.bin");
const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 16;

/* Only 32-bit-per-pixel framebuffers are supported. */
const BYTES_PER_PIXEL: usize = 4;

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
	 * new - Create a terminal for a framebuffer
	 * @base: Framebuffer base pointer
	 * @width: Width in pixels
	 * @height: Height in pixels
	 * @pitch: Bytes per scanline
	 * @bpp: Bits per pixel
	 *
	 * Return: Some(Tty) when the framebuffer is a usable 32-bit-per-pixel
	 * surface, None otherwise. Unsupported formats are rejected instead of
	 * being rendered incorrectly.
	 */
	pub fn new(
		base: *mut u8,
		width: usize,
		height: usize,
		pitch: usize,
		bpp: usize,
	) -> Option<Self> {
		let minimum_pitch = width.checked_mul(BYTES_PER_PIXEL)?;
		/* Ensure framebuffer-size and row-offset arithmetic cannot overflow. */
		pitch.checked_mul(height)?;
		if base.is_null()
			|| (base as usize) % BYTES_PER_PIXEL != 0
			|| bpp != BYTES_PER_PIXEL * 8
			|| width < FONT_WIDTH
			|| height < FONT_HEIGHT
			|| pitch < minimum_pitch
			|| pitch % BYTES_PER_PIXEL != 0
		{
			return None;
		}

		Some(Self {
			fb_base: base,
			fb_width: width,
			fb_height: height,
			fb_pitch: pitch,
			cols: width / FONT_WIDTH,
			rows: height / FONT_HEIGHT,
			x: 0,
			y: 0,
			color_fg: 0x00AAAAAA,
			color_bg: 0x00000000,
		})
	}

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
	 * Renders the character using the 8x16 bitmap font. The cursor is
	 * always clamped inside the framebuffer by the callers, so the pixel
	 * offsets computed here stay within the mapped surface.
	 */
	fn draw_char(&mut self, cx: usize, cy: usize, c: char) {
		let glyph_index = match c {
			' '..='~' => c as usize,
			_ => b'?' as usize,
		};

		let Some(screen_x) = cx.checked_mul(FONT_WIDTH) else {
			return;
		};
		let Some(screen_y) = cy.checked_mul(FONT_HEIGHT) else {
			return;
		};

		/* Defensive bounds check: never write outside the framebuffer. */
		let Some(screen_x_end) = screen_x.checked_add(FONT_WIDTH) else {
			return;
		};
		let Some(screen_y_end) = screen_y.checked_add(FONT_HEIGHT) else {
			return;
		};
		if screen_x_end > self.fb_width || screen_y_end > self.fb_height {
			return;
		}

		let glyph = &FONT_8X16[glyph_index * 16..(glyph_index + 1) * 16];

		for row in 0..FONT_HEIGHT {
			let bits = glyph[row];
			for col in 0..FONT_WIDTH {
				let offset = (screen_y + row) * self.fb_pitch + (screen_x + col) * BYTES_PER_PIXEL;
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

	/*
	 * set_fg_color - Set the foreground color for text output
	 * @color: 0x00RRGGBB format
	 */
	pub fn set_fg_color(&mut self, color: u32) {
		self.color_fg = color;
	}
}

/*
 * set_kernel_fg_color - Set the foreground color for kernel console text
 * @color: 0x00RRGGBB format
 *
 * Call before printing colored output; restore default (0x00AAAAAA) after.
 * Interrupts are disabled while the console lock is held so this is safe
 * from interrupt context.
 */
pub fn set_kernel_fg_color(color: u32) {
	use x86_64::instructions::interrupts;

	interrupts::without_interrupts(|| {
		if let Some(tty) = KERNEL_TTY.lock().as_mut() {
			tty.set_fg_color(color);
		}
	})
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
 * _print - Internal print function for kprint!/kprintln!
 * @args: Format arguments
 *
 * Called by the kprint! and kprintln! macros. Interrupts are disabled for
 * the duration of the write so the console can be used from any context.
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
 * init_console - Initialize the kernel TTY
 * @base: Framebuffer base pointer
 * @width: Width in pixels
 * @height: Height in pixels
 * @pitch: Bytes per scanline
 * @bpp: Bits per pixel
 *
 * Return: true when the console was initialized, false when the framebuffer
 * uses an unsupported format or invalid geometry.
 */
pub unsafe fn init_console(
	base: *mut u8,
	width: usize,
	height: usize,
	pitch: usize,
	bpp: usize,
) -> bool {
	use x86_64::instructions::interrupts;

	let mut tty = match Tty::new(base, width, height, pitch, bpp) {
		Some(tty) => tty,
		None => return false,
	};
	tty.clear();
	interrupts::without_interrupts(|| {
		*KERNEL_TTY.lock() = Some(tty);
	});
	true
}

/*
 * clear_kernel_console - Clear the global kernel text console
 *
 * Interrupts are disabled while the console lock is held.
 */
pub fn clear_kernel_console() {
	use x86_64::instructions::interrupts;

	interrupts::without_interrupts(|| {
		if let Some(tty) = KERNEL_TTY.lock().as_mut() {
			tty.clear();
		}
	})
}
