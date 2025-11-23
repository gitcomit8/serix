#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ulib::{exit, write, STDOUT};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	exit(-1);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	write(STDOUT, b"Hello from a REAL ELF!\n");
	exit(0);
}
