/*
 * console.rs - Console device driver for VFS
 *
 * Implements a console device that writes to serial and framebuffer.
 */

use hal::serial_println;
use vfs::{FileType, INode};

/*
 * struct ConsoleDevice - Virtual console device
 */
pub struct ConsoleDevice;

impl ConsoleDevice {
	/*
	 * new - Create a new console device instance
	 *
	 * Return: A new ConsoleDevice
	 */
	pub fn new() -> Self {
		Self
	}
}

impl INode for ConsoleDevice {
	/*
	 * read - Read from console (not implemented)
	 * @_offset: Offset to read from (unused)
	 * @_buf: Buffer to read into (unused)
	 *
	 * Return: Always returns 0 (no data)
	 */
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
		// TODO: Hookup keyboard input here later
		0
	}

	/*
	 * write - Write to console output
	 * @_offset: Offset to write at (unused)
	 * @buf: Buffer containing data to write
	 *
	 * Writes UTF-8 text to serial console and framebuffer.
	 *
	 * Return: Number of bytes written, or 0 on error
	 */
	fn write(&self, _offset: usize, buf: &[u8]) -> usize {
		if let Ok(s) = core::str::from_utf8(buf) {
			serial_println!("{}", s);
			// fb_println!("{}", s);
			buf.len()
		} else {
			0
		}
	}

	/*
	 * metadata - Get device metadata
	 *
	 * Return: FileType::Device
	 */
	fn metadata(&self) -> FileType {
		FileType::Device
	}
}
