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
 * @cap_id: CapabilityId for this open file (for syscall authorization)
 */
pub struct OpenFile {
	pub inode: Arc<dyn INode>,
	pub offset: Mutex<usize>,
	/// CapabilityId for this open file. Used by the syscall layer to
	/// resolve the file through the caller's cspace during authorization.
	pub cap_id: Option<capability::types::CapabilityId>,
}

/*
 * FD_TABLE - Global file descriptor table keyed by (task_id, fd)
 */
static FD_TABLE: Mutex<BTreeMap<(u64, u64), Arc<OpenFile>>> = Mutex::new(BTreeMap::new());

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
 *
 * Mints an inode capability for the caller and inserts it into the task's
 * C-space at the same index as the fd, so the caller can derive a
 * CapabilityRef from the fd number for all subsequent operations.
 */
pub fn open(task_id: u64, path: &str) -> Option<u64> {
	let inode = vfs::lookup_path(path)?;
	let mut next = NEXT_FD.lock();
	let fd = *next;
	*next += 1;

	let ptr = Arc::as_ptr(&inode) as *const () as usize as u64;
	let cap_type = capability::CapabilityType::Inode { ptr };
	let rights = capability::Rights::READ | capability::Rights::WRITE | capability::Rights::SEEK;
	let cap_handle = capability::CapabilityHandle::generate();

	/* Mint capability record — mint_root inserts it into the global store
	 * and returns the full record including the CapabilityId. */
	let record = capability::global_cap_store().lock().mint_root(cap_handle, cap_type, rights, 0);
	let cap_id = record.id;

	/* Insert into the task's C-space at the same index as the fd.
	 * This gives a deterministic mapping: fd 3 → slot 3, etc. */
	if let Some(task_arc) = task::scheduler::find_task_by_id(task_id) {
		task_arc.lock().cspace.lock().insert(cap_id, false);
	}

	let file = Arc::new(OpenFile {
		inode,
		offset: Mutex::new(0),
		cap_id: Some(cap_id),
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
 * @rights: Rights to grant on this inode
 *
 * Return: Allocated fd number
 */
pub fn insert_inode(task_id: u64, inode: Arc<dyn INode>, rights: capability::Rights) -> u64 {
	let mut table = FD_TABLE.lock();
	let fd = next_fd(task_id, &table);
	let ptr = Arc::as_ptr(&inode) as *const () as usize as u64;
	let cap_id = mint_cap_for_inode(task_id, inode.clone(), ptr, rights);
	table.insert(
		(task_id, fd),
		Arc::new(OpenFile {
			inode,
			offset: Mutex::new(0),
			cap_id,
		}),
	);
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
 *
 * Duplicates the capability into the new fd's slot in the task's C-space.
 */
pub fn dup(task_id: u64, old_fd: u64) -> Option<u64> {
	let mut table = FD_TABLE.lock();
	let file = table.get(&(task_id, old_fd))?.clone();
	let old_cap_id = file.cap_id?;
	let new_fd = next_fd(task_id, &table);
	/* Duplicate the capability into a new slot matching the new fd */
	if let Some(task_arc) = task::scheduler::find_task_by_id(task_id) {
		task_arc.lock().cspace.lock().insert(old_cap_id, false);
	}
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
 *
 * Duplicates the capability into the new fd's slot in the task's C-space.
 */
pub fn dup2(task_id: u64, old_fd: u64, new_fd: u64) -> Option<u64> {
	if old_fd == new_fd {
		/* Verify old_fd exists */
		let table = FD_TABLE.lock();
		return if table.contains_key(&(task_id, old_fd)) {
			Some(new_fd)
		} else {
			None
		};
	}
	let mut table = FD_TABLE.lock();
	let file = table.get(&(task_id, old_fd))?.clone();
	let old_cap_id = file.cap_id?;
	table.remove(&(task_id, new_fd)); /* silently close if open */
	/* Duplicate the capability into a new slot matching the new fd */
	if let Some(task_arc) = task::scheduler::find_task_by_id(task_id) {
		task_arc.lock().cspace.lock().insert(old_cap_id, false);
	}
	table.insert((task_id, new_fd), file);
	Some(new_fd)
}

/*
 * clone_for_task - Copy all fds from src_task to dst_task
 * @src_task: Task to copy from (parent on spawn)
 * @dst_task: Task to copy into (child)
 *
 * Each cloned fd shares the same INode but gets its own offset cursor.
 * Capabilities are NOT cloned — each task must open its own files.
 * If a capability existed, delegate it to the child task.
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
				cap_id: None, // Capabilities delegated below
			});
			(fd, file.cap_id, new_file)
		})
		.collect();
	drop(table);
	let mut table = FD_TABLE.lock();
	for (fd, cap_id, new_file) in entries {
		table.insert((dst_task, fd), new_file);
		// Delegate capability to child task if it existed for source
		if let Some(id) = cap_id {
			if let Some(child_arc) = task::scheduler::find_task_by_id(dst_task) {
				child_arc.lock().cspace.lock().insert(id, false);
			}
		}
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
 * Mints capabilities for stdin, stdout, and stderr so the task can
 * authorize I/O operations.
 */
pub fn init_stdio(task_id: u64) {
	use crate::stdio::{StderrINode, StdinINode, StdoutINode};

	let mut table = FD_TABLE.lock();

	// Stdin — READ only
	let stdin_arc = Arc::new(StdinINode);
	let stdin_ptr = Arc::as_ptr(&stdin_arc) as usize as u64;
	let stdin_cap = mint_cap_for_inode(task_id, stdin_arc, stdin_ptr, capability::Rights::READ);

	// Stdout — WRITE only
	let stdout_arc = Arc::new(StdoutINode);
	let stdout_ptr = Arc::as_ptr(&stdout_arc) as usize as u64;
	let stdout_cap = mint_cap_for_inode(task_id, stdout_arc, stdout_ptr, capability::Rights::WRITE);

	// Stderr — WRITE only
	let stderr_arc = Arc::new(StderrINode);
	let stderr_ptr = Arc::as_ptr(&stderr_arc) as usize as u64;
	let stderr_cap = mint_cap_for_inode(task_id, stderr_arc, stderr_ptr, capability::Rights::WRITE);

	table.insert(
		(task_id, 0),
		Arc::new(OpenFile {
			inode: Arc::new(StdinINode),
			offset: Mutex::new(0),
			cap_id: stdin_cap,
		}),
	);
	table.insert(
		(task_id, 1),
		Arc::new(OpenFile {
			inode: Arc::new(StdoutINode),
			offset: Mutex::new(0),
			cap_id: stdout_cap,
		}),
	);
	table.insert(
		(task_id, 2),
		Arc::new(OpenFile {
			inode: Arc::new(StderrINode),
			offset: Mutex::new(0),
			cap_id: stderr_cap,
		}),
	);
}

/*
 * mint_cap_for_inode - Mint a capability for an inode and insert into task's C-space
 * @task_id: Owning task ID
 * @inode: Arc<dyn INode> to create capability for
 * @ptr: Arc::as_ptr(&inode) cast to u64
 * @rights: Rights to grant
 *
 * Return: CapabilityId of the newly minted capability
 */
fn mint_cap_for_inode(task_id: u64, inode: Arc<dyn INode>, ptr: u64, rights: capability::Rights) -> Option<capability::types::CapabilityId> {
	let cap_type = capability::CapabilityType::Inode { ptr };
	let cap_handle = capability::CapabilityHandle::generate();
	let record = capability::global_cap_store().lock().mint_root(cap_handle, cap_type, rights, 0);
	let cap_id = record.id;

	if let Some(task_arc) = task::scheduler::find_task_by_id(task_id) {
		task_arc.lock().cspace.lock().insert(cap_id, false);
	}

	Some(cap_id)
}
