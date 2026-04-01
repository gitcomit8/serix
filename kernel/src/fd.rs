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

/*
 * insert_inode - Insert an already-constructed INode as a new fd
 * @task_id: Owning task
 * @inode: INode to wrap
 *
 * Return: Allocated fd number
 */
pub fn insert_inode(task_id: u64, inode: Arc<dyn INode>) -> u64 {
	let mut table = FD_TABLE.lock();
	let fd = next_fd(task_id, &table);
	table.insert((task_id, fd), Arc::new(OpenFile {
		inode,
		offset: Mutex::new(0),
	}));
	fd
}

/*
 * next_fd - Find the lowest available fd >= 3 for a task
 * @task_id: Task to allocate fd for
 *
 * Return: Lowest available file descriptor number
 */
fn next_fd(task_id: u64, table: &BTreeMap<(u64, u64), Arc<OpenFile>>) -> u64 {
	let mut fd = 3u64;
	while table.contains_key(&(task_id, fd)) {
		fd += 1;
	}
	fd
}

/*
 * dup - Duplicate a file descriptor to the next available fd
 * @task_id: Calling task's ID
 * @old_fd: File descriptor to duplicate
 *
 * Return: New fd sharing the same OpenFile, None if old_fd not found
 */
pub fn dup(task_id: u64, old_fd: u64) -> Option<u64> {
	let mut table = FD_TABLE.lock();
	let file = table.get(&(task_id, old_fd))?.clone();
	let new_fd = next_fd(task_id, &table);
	table.insert((task_id, new_fd), file);
	Some(new_fd)
}

/*
 * dup2 - Duplicate a file descriptor to a specific fd
 * @task_id: Calling task's ID
 * @old_fd: Source file descriptor
 * @new_fd: Target file descriptor (closed if already open)
 *
 * Return: new_fd on success, None if old_fd not found
 */
pub fn dup2(task_id: u64, old_fd: u64, new_fd: u64) -> Option<u64> {
	if old_fd == new_fd {
		/* Verify old_fd exists */
		let table = FD_TABLE.lock();
		return if table.contains_key(&(task_id, old_fd)) { Some(new_fd) } else { None };
	}
	let mut table = FD_TABLE.lock();
	let file = table.get(&(task_id, old_fd))?.clone();
	table.remove(&(task_id, new_fd)); /* silently close if open */
	table.insert((task_id, new_fd), file);
	Some(new_fd)
}

/*
 * clone_for_task - Copy all fds from src_task to dst_task
 * @src_task: Task to copy from (parent on spawn)
 * @dst_task: Task to copy into (child)
 *
 * Each cloned fd shares the same INode but gets its own offset cursor.
 */
pub fn clone_for_task(src_task: u64, dst_task: u64) {
	let table = FD_TABLE.lock();
	let entries: alloc::vec::Vec<_> = table
		.iter()
		.filter(|&(&(tid, _), _)| tid == src_task)
		.map(|(&(_, fd), file)| {
			/* New OpenFile with independent offset, shared INode */
			let new_file = Arc::new(OpenFile {
				inode: file.inode.clone(),
				offset: spin::Mutex::new(*file.offset.lock()),
			});
			(fd, new_file)
		})
		.collect();
	drop(table);
	let mut table = FD_TABLE.lock();
	for (fd, file) in entries {
		table.insert((dst_task, fd), file);
	}
}

/*
 * cleanup - Remove all file descriptors owned by a task
 * @task_id: Task whose fds to remove
 *
 * Called on task exit to release all open file descriptors.
 */
pub fn cleanup(task_id: u64) {
	FD_TABLE.lock().retain(|&(tid, _), _| tid != task_id);
}

/*
 * init_stdio - Insert fd 0/1/2 into the FD table for a task
 * @task_id: Target task ID
 *
 * Must be called before the task uses read()/write() on stdio fds.
 */
pub fn init_stdio(task_id: u64) {
	use crate::stdio::{StdinINode, StderrINode, StdoutINode};

	let mut table = FD_TABLE.lock();
	table.insert((task_id, 0), Arc::new(OpenFile {
		inode: Arc::new(StdinINode),
		offset: Mutex::new(0),
	}));
	table.insert((task_id, 1), Arc::new(OpenFile {
		inode: Arc::new(StdoutINode),
		offset: Mutex::new(0),
	}));
	table.insert((task_id, 2), Arc::new(OpenFile {
		inode: Arc::new(StderrINode),
		offset: Mutex::new(0),
	}));
}
