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

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};
use core::panic::PanicInfo;

const HEAP_SIZE: usize = 32768;

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
        core::ptr::addr_of_mut!(HEAP).cast::<u8>().add(aligned)
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        /* bump allocator: dealloc is a no-op */
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator;
use ulib::{
	CapabilityRef, ROOT_CAP, STDOUT_CAP, exit, read, serix_close, serix_getpid, serix_getppid,
	serix_open, serix_spawn, serix_wait, write,
};

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
		write(STDOUT_CAP, b"0");
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
		write(STDOUT_CAP, &digits[i..=i]);
	}
}

/*
 * _start - Userspace entry point
 *
 * Demonstrates process lifecycle: prints pid, lists /, then enters echo loop.
 */
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	write(STDOUT_CAP, b"\n=== Serix User Shell (init) ===\n");

	/* Print process IDs */
	write(STDOUT_CAP, b"[init] PID: ");
	let pid = serix_getpid();
	print_u64(pid);
	write(STDOUT_CAP, b", PPID: ");
	let ppid = serix_getppid();
	print_u64(ppid);
	write(STDOUT_CAP, b"\n");

	/* File I/O test: open /hello.txt, read contents, print */
	let fd = serix_open(ROOT_CAP, "/hello.txt");
	if fd >= 3 {
		let fd = fd as usize;
		let mut rbuf = [0u8; 64];
		let cap = CapabilityRef { slot: fd as usize, generation: 1 };
		let n = read(cap, &mut rbuf);
		write(STDOUT_CAP, b"[init] /hello.txt: ");
		write(STDOUT_CAP, &rbuf[..n]);
		write(STDOUT_CAP, b"\n");

		serix_close(CapabilityRef { slot: fd as usize, generation: 1 });
	}

	/* Spawn the rsh shell */
	let child = serix_spawn("/rsh");
	if child > 0 {
		write(STDOUT_CAP, b"[init] spawned rsh, pid=");
		print_u64(child as u64);
		write(STDOUT_CAP, b"\n");
		let (_pid, status) = serix_wait(-1);
		write(STDOUT_CAP, b"[init] rsh exited, status=");
		print_u64(status as u64);
		write(STDOUT_CAP, b"\n");
	} else {
		write(STDOUT_CAP, b"[init] failed to spawn /rsh\n");
	}

	exit(0);
}
