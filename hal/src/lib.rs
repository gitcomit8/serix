#![allow(dead_code)]
#![no_std]

pub mod io;
pub mod serial;
pub use io::*;
pub use serial::{init_serial,serial_print};

pub mod cpu {
    use x86_64::instructions::*;

    #[inline(always)]
	pub fn halt() {
		hlt();
	}

	#[inline(always)]
	pub fn enable_interrupts() {
		interrupts::enable();
	}

	#[inline(always)]
	pub fn disable_interrupts() {
		interrupts::disable();
	}
}
