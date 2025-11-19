#![no_std]
extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;

pub trait INode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;
	fn write(&self, offset: usize, buf: &[u8]) -> usize;
}

pub struct RamFile {
	pub name: String,
	data: spin::Mutex<Vec<u8>>,
}

impl RamFile {
	pub fn new(name: &str) -> Self {
		Self {
			name: String::from(name),
			data: spin::Mutex::new(Vec::new()),
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
}
