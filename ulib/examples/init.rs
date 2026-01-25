#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ulib::{exit, read, write, STDIN, STDOUT};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	exit(-1);
}

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
			// Writeback to screen
			write(STDOUT, &buf);

			// Handle newline aesthetic
			if c == b'\r' || c == b'\n' {
				write(STDOUT, b"$ ");
			}
		}
	}
}
