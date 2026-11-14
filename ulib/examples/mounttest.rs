/*
 * mounttest.rs - Manual QEMU check for SYS_MOUNT and read-only enforcement
 *
 * Not shipped in any image. Boot-tested by embedding it in the VFS and
 * spawning it with a PLATFORM_ADMIN root capability; it mounts /dev/sda
 * read-only, then exercises the write paths that must answer EROFS.
 */

#![no_std]
#![no_main]

use core::alloc::{GlobalAlloc, Layout};
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};

use ulib::{
	exit, serix_mkdir, serix_mount, serix_umount, write, CapabilityRef, MOUNT_RDONLY, ROOT_CAP,
	STDERR_CAP,
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

/* Writes an i64 result so a negative errno encoding is visible in the log. */
fn report(label: &[u8], value: i64) {
	write(STDERR_CAP, label);
	if value < 0 {
		write(STDERR_CAP, b"-");
		print_u64((-value) as u64);
	} else {
		print_u64(value as u64);
	}
	write(STDERR_CAP, b"\n");
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	let admin = CapabilityRef {
		slot: ROOT_CAP.slot,
		generation: ROOT_CAP.generation,
	};

	write(STDERR_CAP, b"[mounttest] begin\n");
	report(b"[mounttest] mount /dev/sda ro -> ", serix_mount(admin, "/dev/sda", "/rotest", MOUNT_RDONLY));
	report(b"[mounttest] mkdir on ro mount -> ", serix_mkdir(ROOT_CAP, "/rotest/denied") as i64);
	report(b"[mounttest] umount /rotest -> ", serix_umount(admin, "/rotest"));
	report(b"[mounttest] mkdir after umount -> ", serix_mkdir(admin, "/afterumount") as i64);
	report(b"[mounttest] umount root -> ", serix_umount(admin, "/"));
	write(STDERR_CAP, b"[mounttest] done\n");
	exit(0);
}
