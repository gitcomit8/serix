/*
 * Graphics Subsystem
 *
 * Provides framebuffer manipulation and console rendering.
 */

#![no_std]

pub mod console;
pub use console::{clear_kernel_console as clear_console, init_console, set_kernel_fg_color};

/*
 * kprint! - Print formatted text to kernel console
 *
 * Usage: kprint!("format string {}", args...)
 */
#[macro_export]
macro_rules! kprint {
    ($($arg:tt)*) => ($crate::console::_print(format_args!($($arg)*)));
}

/*
 * kprintln! - Print formatted text with newline to kernel console
 *
 * Usage: kprintln!("format string {}", args...)
 */
#[macro_export]
macro_rules! kprintln {
    () => ($crate::kprint!("\n"));
    ($($arg:tt)*) => ($crate::kprint!("{}\n", format_args!($($arg)*)));
}

use boot::Framebuffer;
use boot::MemoryRegion;
use boot::MemoryRegionType;

/*
 * write_pixel - Write a pixel to framebuffer
 * @ptr: Framebuffer base pointer
 * @offset: Byte offset to pixel
 * @color: 4-byte color value (BGRA format)
 */
pub unsafe fn write_pixel(ptr: *mut u8, offset: usize, color: &[u8; 4]) {
	unsafe {
		core::ptr::copy_nonoverlapping(color.as_ptr(), ptr.add(offset), 4);
	}
}

/*
 * fill_screen_blue - Fill entire framebuffer with blue color
 * @fb: Framebuffer to fill
 */
pub fn fill_screen_blue(fb: &Framebuffer) {
	let width = fb.width as usize;
	let height = fb.height as usize;
	let pitch = fb.pitch as usize;
	let bpp = fb.bits_per_pixel as usize;
	let ptr = fb.address as *mut u8;
	let blue_pixel = [0xFF, 0x00, 0x00, 0x00]; // BGRA

	for y in 0..height {
		for x in 0..width {
			let offset = y * pitch + x * (bpp / 8);
			unsafe {
				write_pixel(ptr, offset, &blue_pixel);
			}
		}
	}
}

/*
 * draw_memory_map - Draw memory map visualization
 * @fb: Framebuffer to draw on
 * @entries: Array of memory map entries
 *
 * Draws colored bars at the bottom of the screen representing memory regions.
 * Green = usable, yellow = bootloader reclaimable, gray = reserved.
 */
pub fn draw_memory_map(fb: &Framebuffer, entries: &[MemoryRegion]) {
	let width = fb.width as usize;
	let height = fb.height as usize;
	let pitch = fb.pitch as usize;
	let bpp = fb.bits_per_pixel as usize;
	let ptr = fb.address as *mut u8;

	let count = entries.len();
	let max_count = width.min(count);
	let bar_width = width / max_count.max(1);

	for (i, entry) in entries.iter().take(max_count).enumerate() {
		let color = match entry.region_type {
			MemoryRegionType::Usable => [0x00, 0xFF, 0x00, 0x00], // green
			MemoryRegionType::BootloaderReclaimable => {
				[0xFF, 0xFF, 0x00, 0x00]
			} // yellow
			_ => [0x80, 0x80, 0x80, 0x00], // gray
		};
		let x_start = i * bar_width;
		let x_end = (x_start + bar_width).min(width);

		for x in x_start..x_end {
			for y in (height - 40)..height {
				let offset = y * pitch + x * (bpp / 8);
				unsafe {
					write_pixel(ptr, offset, &color);
				}
			}
		}
	}
}
