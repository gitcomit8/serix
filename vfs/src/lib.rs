/*
 * Virtual File System
 *
 * Implements the VFS layer with support for Files, Directories, and Devices
 * Uses Arc for shared ownership of filesystem nodes.
 */

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::mutex::Mutex;
/*
 * enum FileType - Type of VFS node
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
	File,
	Directory,
	Device,
}

/*
 * trait INode - Abstract filesystem node
 *
 * Now supports directory operations (lookup, insert) and metadata
 */
pub trait INode: Send + Sync {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;
	fn write(&self, offset: usize, buf: &[u8]) -> usize;
	fn metadata(&self) -> FileType;

	//Directory operations (default to failing for non-directories)
	fn lookup(&self, _name: &str) -> Option<Arc<dyn INode>> {
		None
	}

	fn insert(&self, _name: &str, _node: Arc<dyn INode>) -> Result<(), &'static str> {
		Err("Not a directory")
	}

	fn size(&self) -> usize {
		00
	}
}

/*
 * struct RamFile - In-memory file implementation
 */
pub struct RamFile {
	name: String,
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

	fn metadata(&self) -> FileType {
		FileType::File
	}

	fn size(&self) -> usize {
		self.data.lock().len()
	}
}

/*
 * struct RamDir - In-memory directory implementation
 */
pub struct RamDir {
	name: String,
	children: Mutex<Vec<(String, Arc<dyn INode>)>>,
}

impl RamDir {
	pub fn new(name: &str) -> Self {
		Self {
			name: String::from(name),
			children: Mutex::new(Vec::new()),
		}
	}
}

impl INode for RamDir {
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize {
		0
	}
	fn write(&self, _offset: usize, _buf: &[u8]) -> usize {
		0
	}
	fn metadata(&self) -> FileType {
		FileType::Directory
	}
	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		let children = self.children.lock();
		children
			.iter()
			.find(|(n, _)| n == name)
			.map(|(_, node)| node.clone())
	}

	fn insert(&self, name: &str, node: Arc<dyn INode>) -> Result<(), &'static str> {
		let mut children = self.children.lock();
		if children.iter().any(|(n, _)| n == name) {
			return Err("File exists");
		}
		children.push((String::from(name), node));
		Ok(())
	}
}
