/*
 * init.rs - Initial userspace program
 *
 * Simple shell that echoes keyboard input to the screen.
 */

#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ulib::{STDIN, STDOUT, exit, read, write};

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
	write(STDOUT, b"$ ");

	let mut buf = [0u8; 1];

	loop {
		// Block until a key is pressed
		let n = read(STDIN, &mut buf);

		if n > 0 {
			let c = buf[0];
			// Echo character back to screen
			write(STDOUT, &buf);

			// Print prompt after newline
			if c == b'\r' || c == b'\n' {
				write(STDOUT, b"$ ");
			}
		}
	}
}
