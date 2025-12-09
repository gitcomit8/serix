use hal::serial_println;
use vfs::{FileType, INode};

pub struct ConsoleDevice;

impl ConsoleDevice {
	pub fn new() -> Self {
		Self
	}
}

impl INode for ConsoleDevice {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
		//TODO: Hookup keyboard input here later
		0
	}

	fn write(&self, _offset: usize, buf: &[u8]) -> usize {
		if let Ok(s) = core::str::from_utf8(buf) {
			serial_println!("{}", s);
			//fb_println!("{}", s);
			buf.len()
		} else {
			0
		}
	}

	fn metadata(&self) -> FileType {
		FileType::Device
	}
}
