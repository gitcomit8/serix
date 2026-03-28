/*
 * lib.rs - Virtual File System
 *
 * Implements the VFS layer with support for files, directories, and devices.
 * Uses Arc for shared ownership of filesystem nodes.
 */

#![no_std]
extern crate alloc;

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::mutex::Mutex;
use spin::Once;

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
 * Provides common interface for files, directories, and devices.
 */
pub trait INode: Send + Sync {
	/*
	 * read - Read data from the node
	 * @offset: Offset to read from
	 * @buf: Buffer to read into
	 *
	 * Return: Number of bytes read
	 */
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;

	/*
	 * write - Write data to the node
	 * @offset: Offset to write to
	 * @buf: Buffer containing data to write
	 *
	 * Return: Number of bytes written
	 */
	fn write(&self, offset: usize, buf: &[u8]) -> usize;

	/*
	 * metadata - Get node metadata
	 *
	 * Return: FileType of this node
	 */
	fn metadata(&self) -> FileType;

	/*
	 * lookup - Look up a child node by name (directories only)
	 * @_name: Name to look up
	 *
	 * Return: Some(node) if found, None otherwise
	 */
	fn lookup(&self, _name: &str) -> Option<Arc<dyn INode>> {
		None
	}

	/*
	 * insert - Insert a child node (directories only)
	 * @_name: Name of child
	 * @_node: Node to insert
	 *
	 * Return: Ok(()) on success, Err on failure
	 */
	fn insert(&self, _name: &str, _node: Arc<dyn INode>) -> Result<(), &'static str> {
		Err("Not a directory")
	}

	/*
	 * size - Get node size in bytes
	 *
	 * Return: Size in bytes
	 */
	fn size(&self) -> usize {
		0
	}
}

/*
 * VFS_ROOT - Global root inode
 */
static VFS_ROOT: Once<Arc<dyn INode>> = Once::new();

/*
 * set_root - Set the global VFS root inode
 * @root: Root directory inode
 */
pub fn set_root(root: Arc<dyn INode>) {
	VFS_ROOT.call_once(|| root);
}

/*
 * lookup_path - Resolve an absolute path to an inode
 * @path: Absolute path (e.g. "/hello.txt" or "/")
 *
 * Return: Some(inode) if found, None otherwise
 */
pub fn lookup_path(path: &str) -> Option<Arc<dyn INode>> {
	let root = VFS_ROOT.get()?;
	if path == "/" || path.is_empty() {
		return Some(Arc::clone(root));
	}
	let mut node: Arc<dyn INode> = Arc::clone(root);
	for component in path.trim_start_matches('/').split('/') {
		if component.is_empty() {
			continue;
		}
		node = node.lookup(component)?;
	}
	Some(node)
}

/*
 * struct RamFile - In-memory file implementation
 * @name: File name
 * @data: File contents
 */
pub struct RamFile {
	name: String,
	data: Mutex<Vec<u8>>,
}

impl RamFile {
	/*
	 * new - Create a new RAM file
	 * @name: File name
	 *
	 * Return: New RamFile instance
	 */
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
 * @name: Directory name
 * @children: List of child nodes
 */
pub struct RamDir {
	name: String,
	children: Mutex<Vec<(String, Arc<dyn INode>)>>,
}

impl RamDir {
	/*
	 * new - Create a new RAM directory
	 * @name: Directory name
	 *
	 * Return: New RamDir instance
	 */
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
