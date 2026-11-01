#![no_std]

use core::arch::asm;

pub mod fmt;
pub mod heap;
pub mod io;

/*
 * Serix System Call Numbers
 *
 * Must stay in sync with kernel/src/syscall.rs.
 * See that file for the full table and per-syscall documentation.
 */
const SYS_EXIT: usize = 0;
const SYS_YIELD: usize = 1;
const SYS_GETPID: usize = 2;
const SYS_GETPPID: usize = 3;
const SYS_SPAWN: usize = 4;
const SYS_WAIT: usize = 5;

const SYS_OPEN: usize = 10;
const SYS_CLOSE: usize = 11;
const SYS_READ: usize = 12;
const SYS_WRITE: usize = 13;
const SYS_SEEK: usize = 14;
const SYS_DUP: usize = 15;
const SYS_DUP2: usize = 16;
const SYS_PIPE: usize = 17;
const SYS_GETDENTS: usize = 18;

const SYS_MKDIR: usize = 20;
const SYS_UNLINK: usize = 21;
const SYS_RMDIR: usize = 22;
const SYS_FORMAT: usize = 23;

const SYS_MMAP: usize = 90;
const SYS_MUNMAP: usize = 91;
const SYS_FILE_READ: usize = 92;

const SYS_SEND: usize = 30;
const SYS_RECV: usize = 31;
const SYS_RECV_BLOCK: usize = 32;
const SYS_CREATE_PORT: usize = 33;
const SYS_CREATE_NOTIFICATION_PORT: usize = 34;
const SYS_NOTIFY: usize = 35;

pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;
pub const STDERR: usize = 2;

/// Stdin capability — slot 0 in every process's C-space (minted by init_stdio).
pub const STDIN_CAP: CapabilityRef = CapabilityRef { slot: 0, generation: 1 };
/// Stdout capability — slot 1 in every process's C-space (minted by init_stdio).
pub const STDOUT_CAP: CapabilityRef = CapabilityRef { slot: 1, generation: 1 };
/// Stderr capability — slot 2 in every process's C-space (minted by init_stdio).
pub const STDERR_CAP: CapabilityRef = CapabilityRef { slot: 2, generation: 1 };

/*
 * syscall3 - Generic syscall wrapper for 3 arguments
 * @nr: System call number
 * @arg1: First argument (passed in rdi)
 * @arg2: Second argument (passed in rsi)
 * @arg3: Third argument (passed in rdx)
 *
 * Follows Linux x86_64 syscall ABI.
 *
 * Return: System call return value
 */
#[inline(always)]
unsafe fn syscall3(nr: usize, arg1: usize, arg2: usize, arg3: usize) -> usize {
	unsafe {
		let ret: usize;
		asm!(
		"syscall",
		in("rax") nr,
		in("rdi") arg1,
		in("rsi") arg2,
		in("rdx") arg3,
		lateout("rax") ret,
		// Syscalls clobber rcx and r11
		out("rcx") _,
		out("r11") _,
		);
		ret
	}
}

/*
 * syscall2 - Generic syscall wrapper for 2 arguments
 */
#[inline(always)]
unsafe fn syscall2(nr: usize, arg1: usize, arg2: usize) -> usize {
	unsafe {
		let ret: usize;
		asm!(
		"syscall",
		in("rax") nr,
		in("rdi") arg1,
		in("rsi") arg2,
		lateout("rax") ret,
		out("rcx") _,
		out("r11") _,
		);
		ret
	}
}

/*
 * syscall1 - Generic syscall wrapper for 1 argument
 * @nr: System call number
 * @arg1: First argument (passed in rdi)
 *
 * Return: System call return value
 */
#[inline(always)]
unsafe fn syscall1(nr: usize, arg1: usize) -> usize {
	unsafe {
		let ret: usize;
		asm!(
		"syscall",
		in("rax") nr,
		in("rdi") arg1,
		lateout("rax") ret,
		out("rcx") _,
		out("r11") _,
		);
		ret
	}
}

/*
 * syscall4 - Generic syscall wrapper for 4 arguments
 */
#[inline(always)]
unsafe fn syscall4(nr: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> usize {
	unsafe {
		let ret: usize;
		asm!(
		"syscall",
		in("rax") nr,
		in("rdi") arg1,
		in("rsi") arg2,
		in("rdx") arg3,
		in("r10") arg4,
		lateout("rax") ret,
		out("rcx") _,
		out("r11") _,
		);
		ret
	}
}

#[inline(always)]
unsafe fn syscall5(nr: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize, arg5: usize) -> usize {
	let ret: usize;
	asm!("syscall", in("rax") nr, in("rdi") arg1, in("rsi") arg2, in("rdx") arg3,
		in("r10") arg4, in("r8") arg5, lateout("rax") ret, out("rcx") _, out("r11") _);
	ret
}

#[inline(always)]
unsafe fn syscall6(nr: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize, arg5: usize, arg6: usize) -> usize {
	let ret: usize;
	asm!("syscall", in("rax") nr, in("rdi") arg1, in("rsi") arg2, in("rdx") arg3,
		in("r10") arg4, in("r8") arg5, in("r9") arg6, lateout("rax") ret,
		out("rcx") _, out("r11") _);
	ret
}

/// Ordinary protected syscall authority. A port/FD/PID is never authority by
/// itself; callers retain the slot generation returned by the grant source.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CapabilityRef { pub slot: usize, pub generation: u32 }

/// Root directory capability granted to all user processes at spawn time.
/// Used to open files relative to / (e.g., /hello.txt).
/// Slot 3 for non-ext4d; slot 5 for ext4d (after IPC caps at 3, 4).
pub const ROOT_CAP: CapabilityRef = CapabilityRef { slot: 3, generation: 1 };

pub const EXT4_REQUEST_CAP: CapabilityRef = CapabilityRef { slot: 3, generation: 1 };
pub const EXT4_REPLY_CAP: CapabilityRef = CapabilityRef { slot: 4, generation: 1 };

/// Root directory capability for ext4d (slot 5, gen 1).
/// Used to open files relative to / (e.g., /dev/sda).
pub const EXT4_ROOT_CAP: CapabilityRef = CapabilityRef { slot: 5, generation: 1 };

/*
 * syscall0 - Generic syscall wrapper for 0 arguments
 */
#[inline(always)]
unsafe fn syscall0(nr: usize) -> usize {
	unsafe {
		let ret: usize;
		asm!(
		"syscall",
		in("rax") nr,
		lateout("rax") ret,
		out("rcx") _,
		out("r11") _,
		);
		ret
	}
}

/*
 * write - Write data to a file capability
 * @cap: File capability reference (slot=fd, generation from open)
 * @buf: Buffer containing data to write
 *
 * Return: Number of bytes written, or negative errno on error
 */
pub fn write(cap: CapabilityRef, buf: &[u8]) -> isize {
	unsafe { syscall4(SYS_WRITE, cap.slot, cap.generation as usize, buf.as_ptr() as usize, buf.len()) as isize }
}

/*
 * read - Read data from a file capability
 * @cap: File capability reference (slot=fd, generation from open)
 * @buf: Buffer to read data into
 *
 * Return: Number of bytes read
 */
pub fn read(cap: CapabilityRef, buf: &mut [u8]) -> usize {
	unsafe { syscall4(SYS_READ, cap.slot, cap.generation as usize, buf.as_mut_ptr() as usize, buf.len()) as usize }
}

/*
 * exit - Terminate the current process
 * @code: Exit status code
 *
 * Does not return.
 */
/*
 * serix_open - Open a file by path
 * @parent_cap: Capability reference to the parent directory
 * @path: Absolute path string (filename relative to parent)
 *
 * Return: fd (>= 3) on success, or negative errno
 */
pub fn serix_open(parent_cap: CapabilityRef, path: &str) -> isize {
	unsafe { syscall4(SYS_OPEN, parent_cap.slot, parent_cap.generation as usize, path.as_ptr() as usize, path.len()) as isize }
}

/*
 * serix_close - Close a file capability
 * @cap: File capability reference (slot=fd, generation from open)
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_close(cap: CapabilityRef) -> isize {
	unsafe { syscall2(SYS_CLOSE, cap.slot, cap.generation as usize) as isize }
}

/*
 * serix_seek - Set file offset
 * @cap: File capability reference
 * @offset: New byte offset
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_seek(cap: CapabilityRef, offset: usize) -> isize {
	unsafe { syscall3(SYS_SEEK, cap.slot, cap.generation as usize, offset) as isize }
}

/*
 * exit - Terminate the current process
 * @code: Exit status code
 *
 * Does not return.
 */
/*
 * serix_mkdir - Create a directory
 * @parent_cap: Capability reference to the parent directory
 * @path: Name of directory to create (relative to parent)
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_mkdir(parent_cap: CapabilityRef, path: &str) -> isize {
	unsafe { syscall4(SYS_MKDIR, parent_cap.slot, parent_cap.generation as usize, path.as_ptr() as usize, path.len()) as isize }
}

/*
 * serix_unlink - Delete a file
 * @parent_cap: Capability reference to the parent directory
 * @path: Name of file to delete (relative to parent)
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_unlink(parent_cap: CapabilityRef, path: &str) -> isize {
	unsafe { syscall4(SYS_UNLINK, parent_cap.slot, parent_cap.generation as usize, path.as_ptr() as usize, path.len()) as isize }
}

/*
 * serix_rmdir - Remove an empty directory
 * @parent_cap: Capability reference to the parent directory
 * @path: Name of directory to remove (relative to parent)
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_rmdir(parent_cap: CapabilityRef, path: &str) -> isize {
	unsafe { syscall4(SYS_RMDIR, parent_cap.slot, parent_cap.generation as usize, path.as_ptr() as usize, path.len()) as isize }
}

/*
 * serix_mmap - Memory-map a file
 * @file_cap: Capability reference to the backing file
 * @offset: Mapping offset (page-aligned)
 * @length: Mapping length
 *
 * Return: virtual address on success, negative errno on error
 */
pub fn serix_mmap(file_cap: CapabilityRef, offset: usize, length: usize) -> isize {
	unsafe { syscall4(SYS_MMAP, file_cap.slot, file_cap.generation as usize, offset, length) as isize }
}

/*
 * serix_munmap - Unmap a VMA region.
 * @file_cap: Capability reference to the original mmap backing file.
 * @addr: Start address of mapping.
 * @length: Length of mapping.
 *
 * The caller must hold a MUNMAP-capable reference to the original file
 * capability used in the mmap call.
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_munmap(file_cap: CapabilityRef, addr: usize, _length: usize) -> isize {
	unsafe { syscall3(SYS_MUNMAP, file_cap.slot, file_cap.generation as usize, addr) as isize }
}

pub fn exit(code: i32) -> ! {
	unsafe {
		syscall1(SYS_EXIT, code as usize);
		loop {
			asm!("hlt");
		}
	}
}

/*
 * IPC message layout (must match ipc::Message in the kernel):
 *   sender_id: u64
 *   id:        u64
 *   len:       u64
 *   data:      [u8; 128]
 * Total: 152 bytes
 */
pub const IPC_MSG_SIZE: usize = 152;
pub const IPC_MAX_DATA: usize = 128;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct IpcMsg {
	pub sender_id: u64,
	pub id: u64,
	pub len: u64,
	pub data: [u8; IPC_MAX_DATA],
}

impl Default for IpcMsg {
	fn default() -> Self {
		Self {
			sender_id: 0,
			id: 0,
			len: 0,
			data: [0; IPC_MAX_DATA],
		}
	}
}

/*
 * send_ipc - Send a message to an IPC port (SYS_SEND = 30)
 * @port: destination port ID
 * @msg:  pointer to IpcMsg
 *
 * Adapts to kernel SYS_SEND ABI: (port, msg_id, data_ptr, data_len).
 * Returns 0 on success, non-zero on error.
 */
pub fn send_ipc(cap: CapabilityRef, msg: &IpcMsg) -> usize {
	unsafe {
		syscall5(
			SYS_SEND,
			cap.slot,
			cap.generation as usize,
			msg.id as usize,
			msg.data.as_ptr() as usize,
			msg.len as usize,
		)
	}
}

/*
 * recv_ipc_blocking - Block until a message arrives on port (SYS_RECV_BLOCK = 32)
 * @cap:  capability reference for the receive port
 * @msg:  output buffer — receives the full IpcMsg struct
 */
pub fn recv_ipc_blocking(cap: CapabilityRef, msg: &mut IpcMsg) {
	unsafe {
		syscall3(SYS_RECV_BLOCK, cap.slot, cap.generation as usize, msg as *mut IpcMsg as usize);
	}
}

/*
 * open - Open a file by path (byte-slice form, for no_std callers)
 * @parent_cap: Capability reference to the parent directory
 * @path:   null-terminated or length-implicit byte slice
 * @_flags: flags (currently unused)
 *
 * Return: fd on success, large value on error
 */
pub fn open(parent_cap: CapabilityRef, path: &[u8], _flags: usize) -> usize {
	/* Strip trailing null if present, kernel uses ptr+len */
	let len = path.iter().position(|&b| b == 0).unwrap_or(path.len());
	unsafe {
		syscall4(SYS_OPEN, parent_cap.slot, parent_cap.generation as usize, path.as_ptr() as usize, len)
	}
}

/*
 * seek - Set file offset
 * @cap:    file capability reference
 * @offset: new byte offset
 *
 * Return: 0 on success, non-zero on error
 */
pub fn seek(cap: CapabilityRef, offset: usize) -> usize {
	unsafe { syscall3(SYS_SEEK, cap.slot, cap.generation as usize, offset) }
}

/*
 * getpid - Return the calling task's ID
 */
pub fn getpid() -> usize {
	unsafe { syscall0(SYS_GETPID) }
}

/*
 * serix_yield - Voluntarily yield the CPU to the scheduler
 */
pub fn serix_yield() {
	unsafe {
		syscall0(SYS_YIELD);
	}
}

/*
 * file_read - Read an entire file into a buffer
 * @parent_cap: Capability reference to the parent directory
 * @path: File path to read
 * @buf: Output buffer
 *
 * Opens the file, reads all contents into buf (capped at buf.len()),
 * and returns the number of bytes read.
 *
 * Return: Bytes read on success, -1 on error (errno encoded)
 */
pub fn file_read(parent_cap: CapabilityRef, path: &str, buf: &mut [u8]) -> isize {
	let ret = unsafe {
		syscall6(SYS_FILE_READ, parent_cap.slot, parent_cap.generation as usize,
			path.as_ptr() as usize, path.len(),
			buf.as_mut_ptr() as usize, buf.len())
	};
	ret as isize
}

/* serix_getpid - Return the calling task's ID */
pub fn serix_getpid() -> u64 {
	unsafe { syscall0(SYS_GETPID) as u64 }
}

/* serix_getppid - Return the parent task's ID */
pub fn serix_getppid() -> u64 {
	unsafe { syscall0(SYS_GETPPID) as u64 }
}

/*
 * serix_spawn - Create a new user process from an ELF on the VFS.
 * @path: Absolute path to the ELF binary
 *
 * Return: child pid (> 0) on success, negative errno on failure
 */
pub fn serix_spawn(path: &str) -> i64 {
	unsafe { syscall2(SYS_SPAWN, path.as_ptr() as usize, path.len()) as i64 }
}

/*
 * serix_wait - Wait for a child process to exit.
 * @pid: Child pid (-1 = any child)
 *
 * Return: (child_pid, exit_status) on success
 */
pub fn serix_wait(pid: i64) -> (i64, i32) {
	let mut status: i32 = 0;
	let ret = unsafe {
		syscall4(
			SYS_WAIT,
			pid as usize,
			&mut status as *mut i32 as usize,
			0,
			0,
		) as i64
	};
	(ret, status)
}

/*
 * serix_getdents - Read directory entries into a buffer.
 * @cap: File capability reference (must be open on a directory)
 * @buf: Buffer to write dirent64 records into
 *
 * Return: bytes written, 0 at EOF, negative errno on error
 */
pub fn serix_getdents(cap: CapabilityRef, buf: &mut [u8]) -> isize {
	unsafe { syscall4(SYS_GETDENTS, cap.slot, cap.generation as usize, buf.as_mut_ptr() as usize, buf.len()) as isize }
}

/* serix_dup - Duplicate fd capability to the next available descriptor */
pub fn serix_dup(cap: CapabilityRef) -> isize {
	unsafe { syscall3(SYS_DUP, cap.slot, cap.generation as usize, 0) as isize }
}

/* serix_dup2 - Duplicate old_fd capability to new_fd */
pub fn serix_dup2(old_cap: CapabilityRef, new_fd: usize) -> isize {
	unsafe { syscall3(SYS_DUP2, old_cap.slot, old_cap.generation as usize, new_fd) as isize }
}

/*
 * serix_pipe - Create a pipe.
 * @parent_cap: Capability reference to a parent directory (CONTROL right).
 * @fds: [read_fd, write_fd] output array.
 * @cap_fds: [cap_slot_r, cap_slot_w] output array.
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_pipe(
	parent_cap: CapabilityRef,
	fds: &mut [u64; 2],
	cap_fds: &mut [u64; 2],
) -> isize {
	unsafe {
		syscall4(
			SYS_PIPE,
			parent_cap.slot,
			parent_cap.generation as usize,
			fds.as_mut_ptr() as usize,
			cap_fds.as_mut_ptr() as usize,
		) as isize
	}
}

/*
 * serix_create_port - Create an IPC port
 * @cap_handle: Output buffer for capability handle (16 bytes / 2 u64)
 *
 * Creates a new IPC port and grants the calling task a capability
 * with both send and receive rights.
 *
 * Return: Port ID on success, negative errno on error
 */
pub fn serix_create_port(cap_handle: &mut [u64; 2]) -> i64 {
	unsafe { syscall1(SYS_CREATE_PORT, cap_handle.as_mut_ptr() as usize) as i64 }
}

/*
 * serix_create_notification_port - Create an async notification port
 * @cap_handle: Output buffer for capability handle (16 bytes / 2 u64)
 * @bitmask: Initial event bitmask to listen for
 *
 * Creates a port that supports async notifications via notify()/check_notification().
 *
 * Return: Port ID on success, negative errno on error
 */
pub fn serix_create_notification_port(cap_handle: &mut [u64; 2], bitmask: u64) -> i64 {
	unsafe { syscall2(SYS_CREATE_NOTIFICATION_PORT, cap_handle.as_mut_ptr() as usize, bitmask as usize) as i64 }
}

/*
 * serix_notify - Send an async notification to a port
 * @cap:  capability reference for the notification port
 * @event_mask: Event bitmask
 *
 * Sets the event bitmask on the target port and wakes any blocked receiver.
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_notify(cap: CapabilityRef, event_mask: u64) -> i64 {
	unsafe { syscall3(SYS_NOTIFY, cap.slot, cap.generation as usize, event_mask as usize) as i64 }
}

/*
 * serix_format - Format a block device with ext4 filesystem.
 * @admin_cap: Capability reference with PLATFORM_ADMIN right.
 * @dev_path: Device path (e.g., "/dev/sda").
 *
 * The kernel derives block_size defaults and inode_count from the device capacity.
 *
 * Returns: 0 on success, negative errno on error
 */
pub fn serix_format(admin_cap: CapabilityRef, dev_path: &str) -> i64 {
	unsafe {
		syscall2(SYS_FORMAT, admin_cap.slot, admin_cap.generation as usize) as i64
	}
}
