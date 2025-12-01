#![no_std]
#![no_main]

use core::panic::PanicInfo;
use ulib::{exit, spawn_thread, write, yield_cpu, STDOUT};

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	exit(-1);
}

#[repr(align(16))]
struct AlignedStack([u8; 4096]);
// Thread stack (4KB) - Must be initialized
static mut THREAD_STACK: AlignedStack = AlignedStack([0; 4096]);

extern "C" fn worker() {
	for _ in 0..3 {
		write(STDOUT, b" [Thread B] Working...\n");
		yield_cpu(); // Yield to let Main run
	}
	write(STDOUT, b" [Thread B] Finished!\n");
	exit(0);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	write(STDOUT, b"Main: Spawning thread...\n");

	unsafe {
		// FIX: Avoid creating a reference to static mut directly.
		// 1. Get raw pointer to the static array
		let stack_ptr = core::ptr::addr_of_mut!(THREAD_STACK) as *mut u8;

		// 2. Create a slice from the raw pointer
		let stack_slice = core::slice::from_raw_parts_mut(stack_ptr, 4096);

		// 3. Pass the slice to spawn_thread
		let _tid = spawn_thread(worker, stack_slice);
	}

	for _ in 0..3 {
		write(STDOUT, b" [Main A] Waiting...\n");
		yield_cpu(); // Yield to let Thread run
	}

	write(STDOUT, b"Main: Done.\n");

	// Spin a bit to let thread finish
	for _ in 0..1000000 {
		core::hint::spin_loop();
	}

	exit(0);
}
