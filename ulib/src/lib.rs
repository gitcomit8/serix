#![no_std]

use core::arch::asm;

const SYS_READ: usize = 0;
const SYS_WRITE: usize = 1;
const SYS_OPEN: usize = 2;
const SYS_CLOSE: usize = 3;
const SYS_SEEK: usize = 8;
const SYS_EXIT: usize = 60;
const SYS_MKDIR: usize = 83;
const SYS_UNLINK: usize = 87;

pub const STDIN: usize = 0;
pub const STDOUT: usize = 1;

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
