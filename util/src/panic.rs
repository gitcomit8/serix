/*
 * Kernel Panic Utilities
 *
 * Provides panic handling and CPU halt functions.
 */

use hal::serial_println;

/*
 * oops - Handle a non-fatal kernel error
 * @msg: Error message to display
 *
 * Prints an error message and halts the CPU.
 */
pub fn oops(msg: &str) {
	serial_println!("[KERNEL OOPS] {}", msg);
	halt_loop();
}

/*
 * halt_loop - Halt the CPU indefinitely
 *
 * Enters an infinite loop with HLT instructions to save power.
 */
pub fn halt_loop() -> ! {
	loop {
		unsafe {
			core::arch::asm!("hlt");
		}
	}
}
