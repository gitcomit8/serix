use hal::serial_println;
pub fn oops(msg: &str) {
	serial_println!("[KERNEL OOPS] {}", msg);
	halt_loop();
}

pub fn halt_loop() -> ! {
	loop {
		unsafe {
			core::arch::asm!("hlt");
		}
	}
}
