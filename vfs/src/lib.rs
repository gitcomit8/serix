/*
 * lib.rs - Virtual File System
 *
 * Implements the VFS layer with a mount table that supports multiple
 * simultaneous filesystem mounts. Longest-prefix matching ensures that
 * specific mounts (e.g. /dev/) shadow the root mount for their subtree.
 *
 * Mount table is kept sorted longest-path-first so the first match is
 * always the most specific one.
 */

#![no_std]
extern crate alloc;

use alloc::string::{String, ToString};
use alloc::sync::Arc;
use alloc::vec::Vec;
use spin::Mutex;

/* ------------------------------------------------------------------ */
/*  FileType                                                            */
/* ------------------------------------------------------------------ */

/*
 * enum FileType - Type of VFS node
 */
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
	File,
	Directory,
	Device,
}

/* ------------------------------------------------------------------ */
/*  INode trait                                                         */
/* ------------------------------------------------------------------ */

/*
 * trait INode - Abstract filesystem node
 *
 * All filesystem drivers implement this trait for their file and
 * directory types. Default implementations return errors or zero so
 * that drivers only need to implement operations they support.
 */
pub trait INode: Send + Sync {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize;
	fn write(&self, offset: usize, buf: &[u8]) -> usize;
	fn metadata(&self) -> FileType;

	fn lookup(&self, _name: &str) -> Option<Arc<dyn INode>> { None }

	fn insert(&self, _name: &str, _node: Arc<dyn INode>) -> Result<(), &'static str> {
		Err("not a directory")
	}

	/*
	 * create_file - Create a new empty file in this directory
	 * @name: Name for the new file
	 *
	 * Returns the INode of the newly created file, which the caller can
	 * immediately write to. Used by kshell `write` when the target does
	 * not exist yet. ext2 allocates an on-disk inode; RamDir wraps RamFile.
	 */
	fn create_file(&self, _name: &str) -> Result<Arc<dyn INode>, &'static str> {
		Err("not a directory")
	}

	fn mkdir(&self, _name: &str) -> Result<(), &'static str> {
		Err("not a directory")
	}

	fn unlink(&self, _name: &str) -> Result<(), &'static str> {
		Err("not a directory")
	}

	fn size(&self) -> usize { 0 }

	fn readdir(&self) -> Option<Vec<(String, FileType)>> { None }
}

/* ------------------------------------------------------------------ */
/*  Mount table                                                         */
/* ------------------------------------------------------------------ */

struct MountEntry {
	path: String,          /* e.g. "/" or "/dev/" — always ends with / */
	root: Arc<dyn INode>,
}

/*
 * MOUNT_TABLE - All active mounts, sorted longest-path-first.
 *
 * Longest-first ordering means the first matching entry in a linear scan
 * is always the most specific mount for any given path.
 */
static MOUNT_TABLE: Mutex<Vec<MountEntry>> = Mutex::new(Vec::new());

/*
 * normalize_mount_path - Ensure path ends with '/' (root stays "/")
 */
fn normalize_mount_path(path: &str) -> String {
	if path == "/" {
		return "/".to_string();
	}
	let p = path.trim_end_matches('/');
	let mut s = String::from(p);
	s.push('/');
	s
}

/*
 * mount - Add or replace a mount point
 * @path:  Mount point path (e.g. "/" or "/dev/")
 * @root:  Root INode of the filesystem to mount here
 */
pub fn mount(path: &str, root: Arc<dyn INode>) {
	let key = normalize_mount_path(path);
	let mut table = MOUNT_TABLE.lock();
	/* Replace existing entry if present */
	if let Some(entry) = table.iter_mut().find(|e| e.path == key) {
		entry.root = root;
		return;
	}
	table.push(MountEntry { path: key, root });
	/* Keep sorted longest-path-first for correct prefix matching */
	table.sort_unstable_by(|a, b| b.path.len().cmp(&a.path.len()));
}

/*
 * umount - Remove a mount point
 * @path: Mount point to remove
 *
 * Returns Err if the path is not currently mounted.
 */
pub fn umount(path: &str) -> Result<(), &'static str> {
	let key = normalize_mount_path(path);
	let mut table = MOUNT_TABLE.lock();
	if let Some(pos) = table.iter().position(|e| e.path == key) {
		table.remove(pos);
		Ok(())
	} else {
		Err("not mounted")
	}
}

/*
 * set_root - Convenience wrapper: mount an inode at "/"
 *
 * Kept for compatibility with existing callers (kernel/src/main.rs).
 */
pub fn set_root(root: Arc<dyn INode>) {
	mount("/", root);
}

/*
 * lookup_path - Resolve an absolute path to an INode
 * @path: Absolute path (must start with '/')
 *
 * Finds the longest matching mount prefix, uses that mount's root as
 * the starting INode, then traverses remaining path components.
 *
 * Return: Some(inode) if found, None if path or any component is absent.
 */
pub fn lookup_path(path: &str) -> Option<Arc<dyn INode>> {
	let table = MOUNT_TABLE.lock();
	if table.is_empty() {
		return None;
	}

	/* Find the longest mount prefix that path starts with */
	let entry = table.iter().find(|e| {
		if e.path == "/" {
			path.starts_with('/')
		} else {
			/* Exact match on mount point or path is inside mount */
			path == e.path.trim_end_matches('/')
				|| path.starts_with(e.path.as_str())
		}
	})?;

	/* Strip the mount prefix to get the remainder to traverse */
	let remainder = if entry.path == "/" {
		path.trim_start_matches('/')
	} else {
		path.strip_prefix(entry.path.as_str())
			.unwrap_or("")
			.trim_start_matches('/')
	};

	if remainder.is_empty() {
		return Some(Arc::clone(&entry.root));
	}

	let mut node = Arc::clone(&entry.root);
	for component in remainder.split('/').filter(|c| !c.is_empty()) {
		node = node.lookup(component)?;
	}
	Some(node)
}

/* ------------------------------------------------------------------ */
/*  RamFile                                                             */
/* ------------------------------------------------------------------ */

/*
 * struct RamFile - In-memory file
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

	/*
	 * new_with_data - Create a RamFile pre-populated with data
	 * @data: Initial file contents
	 *
	 * Used to embed static binaries (e.g. ext4d ELF) into the VFS at boot.
	 */
	pub fn new_with_data(data: &[u8]) -> Self {
		let mut f = Self::new("");
		f.data.lock().extend_from_slice(data);
		f
	}
}

impl INode for RamFile {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		let data = self.data.lock();
		if offset >= data.len() { return 0; }
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

	fn metadata(&self) -> FileType { FileType::File }

	fn size(&self) -> usize { self.data.lock().len() }
}

/* ------------------------------------------------------------------ */
/*  RamDir                                                              */
/* ------------------------------------------------------------------ */

/*
 * struct RamDir - In-memory directory
 */
pub struct RamDir {
	pub name: String,
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
	fn read(&self, _offset: usize, _buf: &mut [u8]) -> usize { 0 }
	fn write(&self, _offset: usize, _buf: &[u8]) -> usize { 0 }
	fn metadata(&self) -> FileType { FileType::Directory }

	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		self.children.lock()
			.iter()
			.find(|(n, _)| n == name)
			.map(|(_, node)| Arc::clone(node))
	}

	fn insert(&self, name: &str, node: Arc<dyn INode>) -> Result<(), &'static str> {
		let mut children = self.children.lock();
		if children.iter().any(|(n, _)| n == name) {
			return Err("file exists");
		}
		children.push((String::from(name), node));
		Ok(())
	}

	fn create_file(&self, name: &str) -> Result<Arc<dyn INode>, &'static str> {
		let node: Arc<dyn INode> = Arc::new(RamFile::new(name));
		self.insert(name, Arc::clone(&node))?;
		Ok(node)
	}

	fn mkdir(&self, name: &str) -> Result<(), &'static str> {
		let dir: Arc<dyn INode> = Arc::new(RamDir::new(name));
		self.insert(name, dir)
	}

	fn unlink(&self, name: &str) -> Result<(), &'static str> {
		let mut children = self.children.lock();
		if let Some(pos) = children.iter().position(|(n, _)| n == name) {
			children.remove(pos);
			Ok(())
		} else {
			Err("not found")
		}
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		Some(
			self.children.lock()
				.iter()
				.map(|(name, node)| (name.clone(), node.metadata()))
				.collect(),
		)
	}
}
