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
