/*
 * argtest.rs - Manual QEMU check for argv passing and SYS_SPAWN arguments
 *
 * Not shipped in any image. Boot-tested by embedding it twice (as /argtest
 * and /argtest2), spawning the first with three arguments, and letting it
 * spawn the second through SYS_SPAWN.
 */

#![no_std]
#![no_main]

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

use ulib::{
	arg, argc, capture_args, exit, serix_spawn_args, serix_wait, write, STDERR_CAP,
};

const HEAP_SIZE: usize = 8192;

#[repr(align(16))]
struct HeapStorage([u8; HEAP_SIZE]);

static mut HEAP: HeapStorage = HeapStorage([0u8; HEAP_SIZE]);
static HEAP_POS: AtomicUsize = AtomicUsize::new(0);

struct BumpAllocator;

unsafe impl GlobalAlloc for BumpAllocator {
	unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
		let pos = HEAP_POS.load(Ordering::Relaxed);
		let aligned = (pos + layout.align() - 1) & !(layout.align() - 1);
		let new_pos = aligned + layout.size();
		if new_pos > HEAP_SIZE {
			return core::ptr::null_mut();
		}
		HEAP_POS.store(new_pos, Ordering::Relaxed);
		unsafe { core::ptr::addr_of_mut!(HEAP).cast::<u8>().add(aligned) }
	}

	unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {}
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
	exit(-1);
}

fn print_u64(mut val: u64) {
	if val == 0 {
		write(STDERR_CAP, b"0");
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
		write(STDERR_CAP, &digits[i..=i]);
	}
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	/* Must be the first thing the program does: the kernel places argc at
	 * the entry stack pointer. */
	let rsp: usize;
	unsafe {
		core::arch::asm!("mov {}, rsp", out(reg) rsp, options(nomem, nostack, preserves_flags));
		capture_args(rsp);
	}

	write(STDERR_CAP, b"[argtest] argc=");
	print_u64(argc() as u64);
	write(STDERR_CAP, b"\n");
	for i in 0..argc() {
		write(STDERR_CAP, b"[argtest] argv[");
		print_u64(i as u64);
		write(STDERR_CAP, b"]=");
		if let Some(bytes) = arg(i) {
			write(STDERR_CAP, bytes);
		}
		write(STDERR_CAP, b"\n");
	}

	/* Only the parent spawns; the child would otherwise recurse forever. */
	let is_parent = match arg(0) {
		Some(name) => name == b"/argtest",
		None => false,
	};
	if is_parent {
		let child = serix_spawn_args("/argtest2", &["argtest2", "from-parent"]);
		write(STDERR_CAP, b"[argtest] spawn -> ");
		if child < 0 {
			write(STDERR_CAP, b"failed\n");
		} else {
			print_u64(child as u64);
			write(STDERR_CAP, b"\n");
			let (_pid, status) = serix_wait(-1);
			write(STDERR_CAP, b"[argtest] child status=");
			print_u64(status as u64);
			write(STDERR_CAP, b"\n");
		}
	}

	write(STDERR_CAP, b"[argtest] done\n");
	exit(0);
}
