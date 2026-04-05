/*
 * stdio.rs - Standard I/O VFS INode implementations
 *
 * Provides INode-backed file descriptors for stdin (fd 0),
 * stdout (fd 1), and stderr (fd 2).
 */

use vfs::{FileType, INode};

/*
 * stdin_inode - Terminal input via PS/2 keyboard
 *
 * Characters are buffered by the PS/2 interrupt handler in keyboard::INPUT_BUF.
 * We spin-yield until a key is available so other tasks can run while waiting.
 */
pub struct StdinINode;

impl INode for StdinINode {
	fn read(&self, _offset: usize, buf: &mut [u8]) -> usize {
		if buf.is_empty() {
			return 0;
		}
		loop {
			if let Some(b) = keyboard::pop_key() {
				buf[0] = b;
				return 1;
			}
			/* Yield to other tasks while waiting for input */
			x86_64::instructions::interrupts::enable();
			core::hint::spin_loop();
			x86_64::instructions::interrupts::disable();
		}
	}

	fn write(&self, _offset: usize, _buf: &[u8]) -> usize {
		0
	}

	fn metadata(&self) -> FileType {
		FileType::Device
	}
}

/* stdout_inode - Framebuffer console + serial */
pub struct StdoutINode;

impl INode for StdoutINode {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
		0
	}

	fn write(&self, _offset: usize, buf: &[u8]) -> usize {
		match core::str::from_utf8(buf) {
			Ok(s) => {
				hal::serial_print!("{}", s);
				graphics::console::_print(format_args!("{}", s));
				buf.len()
			}
			Err(_) => 0,
		}
	}

	fn metadata(&self) -> FileType {
		FileType::Device
	}
}

/* stderr_inode - Serial only */
pub struct StderrINode;

impl INode for StderrINode {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
		0
	}

	fn write(&self, _offset: usize, buf: &[u8]) -> usize {
		match core::str::from_utf8(buf) {
			Ok(s) => {
				hal::serial_print!("{}", s);
				buf.len()
			}
			Err(_) => 0,
		}
	}

	fn metadata(&self) -> FileType {
		FileType::Device
	}
}
