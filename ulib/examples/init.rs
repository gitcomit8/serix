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
use ulib::{STDIN, STDOUT, exit, read, serix_close, serix_getdents64, serix_getpid, serix_getppid, serix_open, write};

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

	/* List root directory */
	write(STDOUT, b"\n[init] Directory listing of /:\n");
	let dirfd = serix_open("/");
	if dirfd >= 3 {
		let dirfd = dirfd as usize;
		let mut dentbuf = [0u8; 4096];
		let mut bytes_read = serix_getdents64(dirfd, &mut dentbuf);

		while bytes_read > 0 {
			/* Parse and print dirent64 entries */
			let mut offset = 0;
			while offset < bytes_read as usize {
				if offset + 19 > dentbuf.len() {
					break;
				}

				/* dirent64: ino(8) + off(8) + reclen(2) + type(1) + name */
				let reclen = u16::from_le_bytes([dentbuf[offset + 16], dentbuf[offset + 17]]) as usize;
				if reclen == 0 {
					break;
				}

				let type_byte = dentbuf[offset + 18];

				/* Find null terminator in name (starts at offset 19) */
				let name_start = offset + 19;
				let mut name_len = 0;
				while name_start + name_len < offset + reclen && dentbuf[name_start + name_len] != 0 {
					name_len += 1;
				}

				write(STDOUT, b"  ");
				write(STDOUT, &dentbuf[name_start..name_start + name_len]);
				write(STDOUT, b" (");
				match type_byte {
					4 => write(STDOUT, b"DIR"),
					8 => write(STDOUT, b"FILE"),
					10 => write(STDOUT, b"LINK"),
					_ => write(STDOUT, b"?"),
				};
				write(STDOUT, b")\n");

				offset += reclen;
			}

			/* Try to read more entries */
			bytes_read = serix_getdents64(dirfd, &mut dentbuf);
			if bytes_read == 0 {
				break;
			}
		}

		serix_close(dirfd);
	} else {
		write(STDOUT, b"[init] Failed to open /\n");
	}

	/* Echo shell */
	write(STDOUT, b"\n[init] Enter shell (Ctrl+C or close to exit):\n$ ");

	let mut buf = [0u8; 1];

	loop {
		let n = read(STDIN, &mut buf);

		if n > 0 {
			let c = buf[0];
			write(STDOUT, &buf);

			if c == b'\r' || c == b'\n' {
				write(STDOUT, b"$ ");
			}
		}
	}
}
