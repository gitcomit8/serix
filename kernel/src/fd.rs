/*
 * fd.rs - Global File Descriptor Table
 *
 * Maps (task_id, fd) pairs to open file state. FDs 0-2 are reserved
 * (stdin, stdout, stderr); user files start at fd 3.
 */

extern crate alloc;

use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use spin::Mutex;
use vfs::INode;

/*
 * struct OpenFile - Per-fd state
 * @inode: VFS node backing this descriptor
 * @offset: Current read/write cursor position
 */
pub struct OpenFile {
	pub inode: Arc<dyn INode>,
	pub offset: Mutex<usize>,
}

/*
 * FD_TABLE - Global file descriptor table keyed by (task_id, fd)
 */
static FD_TABLE: Mutex<BTreeMap<(u64, u64), Arc<OpenFile>>> =
	Mutex::new(BTreeMap::new());

/*
 * NEXT_FD - Per-task next fd counter (simple global counter for now)
 */
static NEXT_FD: Mutex<u64> = Mutex::new(3);

/*
 * open - Open a VFS path and return a file descriptor
 * @task_id: Calling task's ID
 * @path: Absolute path to open
 *
 * Return: fd on success, None if path not found
 */
pub fn open(task_id: u64, path: &str) -> Option<u64> {
	let inode = vfs::lookup_path(path)?;
	let mut next = NEXT_FD.lock();
	let fd = *next;
	*next += 1;

	let file = Arc::new(OpenFile {
		inode,
		offset: Mutex::new(0),
	});

	FD_TABLE.lock().insert((task_id, fd), file);
	Some(fd)
}

/*
 * close - Close a file descriptor
 * @task_id: Calling task's ID
 * @fd: File descriptor to close
 *
 * Return: true if fd existed and was closed
 */
pub fn close(task_id: u64, fd: u64) -> bool {
	FD_TABLE.lock().remove(&(task_id, fd)).is_some()
}

/*
 * get - Look up an open file descriptor
 * @task_id: Calling task's ID
 * @fd: File descriptor
 *
 * Return: Reference to OpenFile if fd is valid
 */
pub fn get(task_id: u64, fd: u64) -> Option<Arc<OpenFile>> {
	FD_TABLE.lock().get(&(task_id, fd)).cloned()
}

/*
 * seek - Set the cursor position for a file descriptor
 * @task_id: Calling task's ID
 * @fd: File descriptor
 * @offset: New cursor position
 *
 * Return: true if fd existed
 */
pub fn seek(task_id: u64, fd: u64, offset: usize) -> bool {
	if let Some(file) = get(task_id, fd) {
		*file.offset.lock() = offset;
		true
	} else {
		false
	}
}
