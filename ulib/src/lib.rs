#![no_std]

use core::arch::asm;

const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_CLOSE: usize = 3;
const SYS_SEEK: usize = 8;
const SYS_GETPID: usize = 39;
const SYS_EXIT: usize = 60;
const SYS_WAIT4: usize = 61;
const SYS_MKDIR: usize = 83;
const SYS_UNLINK: usize = 87;
const SYS_GETPPID: usize = 110;
const SYS_DUP: usize = 32;
const SYS_DUP2: usize = 33;
const SYS_GETDENTS64: usize = 217;
const SYS_PIPE2: usize = 293;
const SYS_SPAWN: usize = 400;

pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;
pub const STDERR: usize = 2;

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
 * write - Write data to a file descriptor
 * @fd: File descriptor (should be STDOUT)
 * @buf: Buffer containing data to write
 *
 * Return: Number of bytes written, or negative errno on error
 */
pub fn write(fd: usize, buf: &[u8]) -> isize {
	unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) as isize }
}

/*
 * read - Read data from a file descriptor
 * @fd: File descriptor (should be STDIN)
 * @buf: Buffer to read data into
 *
 * Return: Number of bytes read
 */
pub fn read(fd: usize, buf: &mut [u8]) -> usize {
	unsafe { syscall3(SYS_READ, fd, buf.as_mut_ptr() as usize, buf.len() as usize) as usize }
}

/*
 * exit - Terminate the current process
 * @code: Exit status code
 *
 * Does not return.
 */
/*
 * serix_open - Open a file by path
 * @path: Absolute path string
 *
 * Return: fd (>= 3) on success, or negative errno
 */
pub fn serix_open(path: &str) -> isize {
	unsafe {
		syscall2(SYS_OPEN, path.as_ptr() as usize, path.len()) as isize
	}
}

/*
 * serix_close - Close a file descriptor
 * @fd: File descriptor to close
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_close(fd: usize) -> isize {
	unsafe { syscall1(SYS_CLOSE, fd) as isize }
}

/*
 * serix_seek - Set file offset
 * @fd: File descriptor
 * @offset: New byte offset
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_seek(fd: usize, offset: usize) -> isize {
	unsafe { syscall2(SYS_SEEK, fd, offset) as isize }
}

/*
 * exit - Terminate the current process
 * @code: Exit status code
 *
 * Does not return.
 */
/*
 * serix_mkdir - Create a directory
 * @path: Absolute path of directory to create
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_mkdir(path: &str) -> isize {
	unsafe {
		syscall2(SYS_MKDIR, path.as_ptr() as usize, path.len()) as isize
	}
}

/*
 * serix_unlink - Delete a file
 * @path: Absolute path of file to delete
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_unlink(path: &str) -> isize {
	unsafe {
		syscall2(SYS_UNLINK, path.as_ptr() as usize, path.len()) as isize
	}
}

pub fn exit(code: i32) -> ! {
	unsafe {
		syscall1(SYS_EXIT, code as usize);
		loop {
			asm!("hlt");
		}
	}
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
	unsafe {
		syscall2(SYS_SPAWN, path.as_ptr() as usize, path.len()) as i64
	}
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
			SYS_WAIT4,
			pid as usize,
			&mut status as *mut i32 as usize,
			0,
			0,
		) as i64
	};
	(ret, status)
}

/*
 * serix_getdents64 - Read directory entries into a buffer.
 * @fd: Open directory fd
 * @buf: Buffer to write dirent64 records into
 *
 * Return: bytes written, 0 at EOF, negative errno on error
 */
pub fn serix_getdents64(fd: usize, buf: &mut [u8]) -> isize {
	unsafe {
		syscall3(SYS_GETDENTS64, fd, buf.as_mut_ptr() as usize, buf.len()) as isize
	}
}

/* serix_dup - Duplicate fd to the next available descriptor */
pub fn serix_dup(fd: usize) -> isize {
	unsafe { syscall1(SYS_DUP, fd) as isize }
}

/* serix_dup2 - Duplicate old_fd to new_fd */
pub fn serix_dup2(old_fd: usize, new_fd: usize) -> isize {
	unsafe { syscall2(SYS_DUP2, old_fd, new_fd) as isize }
}

/*
 * serix_pipe - Create a pipe.
 * @fds: [read_fd, write_fd] output array
 *
 * Return: 0 on success, negative errno on error
 */
pub fn serix_pipe(fds: &mut [u64; 2]) -> isize {
	unsafe { syscall2(SYS_PIPE2, fds.as_mut_ptr() as usize, 0) as isize }
}
