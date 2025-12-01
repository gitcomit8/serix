#![no_std]

use core::arch::asm;

const SYS_WRITE: usize = 1;
const SYS_EXIT: usize = 60;
const SYS_CLONE: usize = 56;

pub const STDOUT: usize = 1;

/*
 * spawn_thread - Create a new thread
 * @func: Function to run in the new thread
 * @stack: Pointer to the top of the stack for the new thread
 */
pub fn spawn_thread(func: extern "C" fn(), stack: &mut [u8]) -> usize {
	let stack_top = unsafe { stack.as_mut_ptr().add(stack.len()) as usize };
	let func_addr = func as usize;

	unsafe { syscall3(SYS_CLONE, 0, stack_top, func_addr) }
}

pub fn yield_cpu() {
	unsafe {
		syscall1(24, 0);
	}
}

/*
 * syscall3 - Generic syscall wrapper for 3 arguments
 * Follows Linux x86_64 API:
 * - NR: rax
 * - Arg1: rdi
 * - Arg2: rsi
 * - Arg3: rdx
 */

#[inline(always)]
unsafe fn syscall3(nr: usize, arg1: usize, arg2: usize, arg3: usize) -> usize {
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

#[inline(always)]
unsafe fn syscall1(nr: usize, arg1: usize) -> usize {
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
