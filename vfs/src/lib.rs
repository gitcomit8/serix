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

pub mod page_cache;

use alloc::collections::BTreeMap;
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

	fn lookup(&self, _name: &str) -> Option<Arc<dyn INode>> {
		None
	}

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

	/*
	 * rmdir - Remove an empty directory
	 * @name: Name of the directory to remove
	 *
	 * The directory must be empty (only . and .. entries).
	 * Default implementation returns ENOTDIR.
	 */
	fn rmdir(&self, _name: &str) -> Result<(), &'static str> {
		Err("not a directory")
	}

	/*
	 * ino - Return the inode ID for caching purposes
	 *
	 * Returns None by default. Filesystem-backed inodes should
	 * return Some(ino) so the page cache can index pages.
	 */
	fn ino(&self) -> Option<u32> {
		None
	}

	fn size(&self) -> usize {
		0
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		None
	}

	/*
	 * is_read_only - Report whether writes through this node are refused
	 *
	 * A mount created read-only wraps its root in ReadOnlyINode, which
	 * answers true here so a syscall can return EROFS instead of a short
	 * write. Nodes under a writable mount keep the default.
	 */
	fn is_read_only(&self) -> bool {
		false
	}
}

/*
 * ReadOnlyINode - Write-refusing view over a mounted filesystem
 *
 * Wraps the root of a read-only mount and wraps every child returned by
 * lookup(), so no path into the mount can reach a writable node. Reads,
 * metadata, and directory listings pass through unchanged.
 */
pub struct ReadOnlyINode {
	inner: Arc<dyn INode>,
}

impl ReadOnlyINode {
	pub fn new(inner: Arc<dyn INode>) -> Self {
		Self { inner }
	}
}

impl INode for ReadOnlyINode {
	fn read(&self, offset: usize, buf: &mut [u8]) -> usize {
		self.inner.read(offset, buf)
	}

	fn write(&self, _offset: usize, _buf: &[u8]) -> usize {
		0
	}

	fn metadata(&self) -> FileType {
		self.inner.metadata()
	}

	fn lookup(&self, name: &str) -> Option<Arc<dyn INode>> {
		self.inner
			.lookup(name)
			.map(|child| Arc::new(ReadOnlyINode::new(child)) as Arc<dyn INode>)
	}

	fn insert(&self, _name: &str, _node: Arc<dyn INode>) -> Result<(), &'static str> {
		Err("Read-only filesystem")
	}

	fn create_file(&self, _name: &str) -> Result<Arc<dyn INode>, &'static str> {
		Err("Read-only filesystem")
	}

	fn mkdir(&self, _name: &str) -> Result<(), &'static str> {
		Err("Read-only filesystem")
	}

	fn unlink(&self, _name: &str) -> Result<(), &'static str> {
		Err("Read-only filesystem")
	}

	fn rmdir(&self, _name: &str) -> Result<(), &'static str> {
		Err("Read-only filesystem")
	}

	fn ino(&self) -> Option<u32> {
		self.inner.ino()
	}

	fn size(&self) -> usize {
		self.inner.size()
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		self.inner.readdir()
	}

	fn is_read_only(&self) -> bool {
		true
	}
}

/* ------------------------------------------------------------------ */
/*  Mount table                                                         */
/* ------------------------------------------------------------------ */

/*
 * MOUNT_TABLE - All active mounts, keyed by mount-point path (BTreeMap
 * preserves lexicographic order; longest-path-first matching is done
 * by iterating in reverse, so the first match is the most specific).
 *
 * E.g. "/dev/sda/" sorts after "/" — reverse iteration finds "/dev/sda/"
 * before "/" for paths under /dev/sda/.
 */
struct Mount {
	root: Arc<dyn INode>,
	read_only: bool,
}

static MOUNT_TABLE: Mutex<BTreeMap<String, Mount>> = Mutex::new(BTreeMap::new());

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
 * mount - Add or replace a writable mount point
 * @path:  Mount point path (e.g. "/" or "/dev/")
 * @root:  Root INode of the filesystem to mount here
 */
pub fn mount(path: &str, root: Arc<dyn INode>) {
	mount_with_flags(path, root, false);
}

/*
 * mount_with_flags - Add or replace a mount point
 * @path:      Mount point path
 * @root:      Root INode of the filesystem to mount here
 * @read_only: Refuse every write through the mount
 *
 * Replacing an existing mount point keeps earlier handles valid: callers
 * that already resolved a node keep using their Arc, and only new lookups
 * see the replacement. A read-only mount stores its root wrapped in
 * ReadOnlyINode, so enforcement does not depend on which syscall is used.
 */
pub fn mount_with_flags(path: &str, root: Arc<dyn INode>, read_only: bool) {
	let key = normalize_mount_path(path);
	let root: Arc<dyn INode> = if read_only {
		Arc::new(ReadOnlyINode::new(root))
	} else {
		root
	};
	let entry = Mount { root, read_only };
	let mut table = MOUNT_TABLE.lock();
	if let Some(existing) = table.get_mut(&key) {
		*existing = entry;
		return;
	}
	table.insert(key, entry);
}

/*
 * is_read_only - Report whether writes to a path would be refused
 * @path: Absolute path to test
 *
 * Uses the same longest-prefix match as lookup_path(), so the answer is
 * about the mount that owns the path rather than the deepest one that
 * merely shares a prefix.
 */
pub fn is_read_only(path: &str) -> bool {
	let table = MOUNT_TABLE.lock();
	table
		.iter()
		.rev()
		.find(|(p, _)| {
			let p = p.as_str();
			if p == "/" {
				path.starts_with('/')
			} else {
				path == p.trim_end_matches('/') || path.starts_with(p)
			}
		})
		.map(|(_, m)| m.read_only)
		.unwrap_or(false)
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
	if table.remove(&key).is_some() {
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
 * Finds the longest matching mount prefix by iterating the BTreeMap
 * in reverse (longest keys first), then traverses the remaining path
 * components from that mount's root INode.
 *
 * Return: Some(inode) if found, None if path or any component is absent.
 */
pub fn lookup_path(path: &str) -> Option<Arc<dyn INode>> {
	let table = MOUNT_TABLE.lock();

	/* Find the longest mount prefix that path starts with (reverse = longest first) */
	let (mount_path, mount) = table.iter().rev().find(|(p, _)| {
		let p = p.as_str();
		if p == "/" {
			path.starts_with('/')
		} else {
			path == p.trim_end_matches('/') || path.starts_with(p)
		}
	})?;
	let root = &mount.root;

	/* Strip the mount prefix to get the remainder to traverse */
	let remainder = if *mount_path == "/" {
		path.trim_start_matches('/')
	} else {
		path.strip_prefix(mount_path.as_str()).unwrap_or("").trim_start_matches('/')
	};

	if remainder.is_empty() {
		return Some(Arc::clone(root));
	}

	let mut node = Arc::clone(root);
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
		self.children
			.lock()
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

	fn rmdir(&self, name: &str) -> Result<(), &'static str> {
		/* Check if it's a directory and empty */
		let children = self.children.lock();
		if let Some(pos) = children.iter().position(|(n, _)| n == name) {
			if children[pos].1.metadata() != FileType::Directory {
				return Err("not a directory");
			}
			/* RamDir doesn't track children of children, so treat as empty */
			drop(children);
			let mut children = self.children.lock();
			children.remove(pos);
			Ok(())
		} else {
			Err("not found")
		}
	}

	fn readdir(&self) -> Option<Vec<(String, FileType)>> {
		Some(
			self.children
				.lock()
				.iter()
				.map(|(name, node)| (name.clone(), node.metadata()))
				.collect(),
		)
	}
}
