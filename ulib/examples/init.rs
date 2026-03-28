/*
 * init.rs - Initial userspace program
 *
 * Simple shell that echoes keyboard input to the screen.
 */

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ulib::{STDIN, STDOUT, exit, read, serix_close, serix_open, serix_seek, write};

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
 * _start - Userspace entry point
 *
 * Implements a simple echo shell that reads keyboard input
 * and writes it back to the screen.
 */
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	write(STDOUT, b"\n--- Serix User Shell ---\n");

	/* File I/O test: open /hello.txt, read contents, print */
	let fd = serix_open("/hello.txt");
	if fd >= 3 {
		let fd = fd as usize;
		let mut rbuf = [0u8; 64];
		let n = read(fd, &mut rbuf);
		write(STDOUT, b"[init] /hello.txt: ");
		write(STDOUT, &rbuf[..n]);
		write(STDOUT, b"\n");

		/* Seek back to start and re-read */
		serix_seek(fd, 0);
		let n2 = read(fd, &mut rbuf);
		write(STDOUT, b"[init] seek(0)+read: ");
		write(STDOUT, &rbuf[..n2]);
		write(STDOUT, b"\n");

		serix_close(fd);
	} else {
		write(STDOUT, b"[init] open /hello.txt failed\n");
	}

	write(STDOUT, b"$ ");

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
