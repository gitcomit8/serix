/*
 * init.rs - Initial userspace program
 *
 * Demonstrates process management syscalls:
 * - getpid/getppid: query task IDs
 * - getdents64: directory listing
 * - spawn/wait4: process creation and reaping
 */

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ulib::{STDOUT, exit, read, serix_close, serix_getpid, serix_getppid, serix_open, serix_spawn,
	serix_wait, write};

/*
 * panic - User panic handler
 * @_info: Panic information (unused)
 *
 * Exits the process with error code -1.
 */
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	exit(-1);
}

/*
 * print_u64 - Write a decimal u64 to stdout
 */
fn print_u64(mut val: u64) {
	if val == 0 {
		write(STDOUT, b"0");
		return;
	}

	let mut digits = [0u8; 20];
	let mut i = 0;

	while val > 0 {
		digits[i] = (val % 10) as u8 + b'0';
		val /= 10;
		i += 1;
	}

	while i > 0 {
		i -= 1;
		write(STDOUT, &digits[i..=i]);
	}
}

/*
 * _start - Userspace entry point
 *
 * Demonstrates process lifecycle: prints pid, lists /, then enters echo loop.
 */
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	write(STDOUT, b"\n=== Serix User Shell (init) ===\n");

	/* Print process IDs */
	write(STDOUT, b"[init] PID: ");
	let pid = serix_getpid();
	print_u64(pid);
	write(STDOUT, b", PPID: ");
	let ppid = serix_getppid();
	print_u64(ppid);
	write(STDOUT, b"\n");

	/* File I/O test: open /hello.txt, read contents, print */
	let fd = serix_open("/hello.txt");
	if fd >= 3 {
		let fd = fd as usize;
		let mut rbuf = [0u8; 64];
		let n = read(fd, &mut rbuf);
		write(STDOUT, b"[init] /hello.txt: ");
		write(STDOUT, &rbuf[..n]);
		write(STDOUT, b"\n");

		serix_close(fd);
	}

	/* Spawn the rsh shell */
	let child = serix_spawn("/rsh");
	if child > 0 {
		write(STDOUT, b"[init] spawned rsh, pid=");
		print_u64(child as u64);
		write(STDOUT, b"\n");
		let (_pid, status) = serix_wait(-1);
		write(STDOUT, b"[init] rsh exited, status=");
		print_u64(status as u64);
		write(STDOUT, b"\n");
	} else {
		write(STDOUT, b"[init] failed to spawn /rsh\n");
	}

	exit(0);
}
