#![no_std]

use core::arch::asm;

const SYS_WRITE: usize = 1;
const SYS_EXIT: usize = 60;

pub const STDOUT: usize = 1;

/*
 * syscall3 - Generic syscall wrapper for 3 arguments
 * Follows Linux x86_64 API:
 * - NR: rax
 * - Arg1: rdi
 * - Arg2: rsi
 * - Arg3: rdx
 */

#[inline(always)]
unsafe fn syscall3(nr: usize, arg1: usize, arg2: usize, arg3: usize) -> usize { unsafe {
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
}}

#[inline(always)]
unsafe fn syscall1(nr: usize, arg1: usize) -> usize { unsafe {
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
}}

// --- The "libc" Stubs ---
pub fn write(fd: usize, buf: &[u8]) -> isize {
	unsafe { syscall3(SYS_WRITE, fd, buf.as_ptr() as usize, buf.len()) as isize }
}

pub fn exit(code: i32) -> ! {
	unsafe {
		syscall1(SYS_EXIT, code as usize);
		loop {
			asm!("hlt");
		}
	}
}
