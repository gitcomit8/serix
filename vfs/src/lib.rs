#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/*
 * trait INode - Abstract filesystem node
 *
 * Added size() method to retrieve file size.
 */
pub trait INode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;
	fn write(&self, offset: usize, buf: &[u8]) -> usize;
	fn size(&self) -> usize;
}

/*
 * struct RamFile - In-memory file implementation
 */
pub struct RamFile {
	pub name: String,
	data: Mutex<Vec<u8>>,
}

impl RamFile {
	pub fn new(name: &str) -> Self {
		Self {
			name: String::from(name),
			data: Mutex::new(Vec::new()),
		}
	}
}

impl INode for RamFile {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		let data = self.data.lock();
		if offset >= data.len() {
			return 0;
		}
		let len = core::cmp::min(buf.len(), data.len() - offset);
		buf[..len].copy_from_slice(&data[offset..offset + len]);
		len
	}

	fn write(&self, offset: usize, buf: &[u8]) -> usize {
		let mut data = self.data.lock();
		if offset + buf.len() > data.len() {
			data.resize(offset + buf.len(), 0);
		}
		data[offset..offset + buf.len()].copy_from_slice(buf);
		buf.len()
	}

	/* Implement size() to return the vector length */
	fn size(&self) -> usize {
		self.data.lock().len()
	}
}
