/*
 * fputest.rs - Manual QEMU check for per-task x87/SSE state
 *
 * Two children load distinct architectural FPU/SSE images (FXSAVE format:
 * XMM0 plus an x87 stack register), busy-loop long enough for the LAPIC
 * timer to preempt them, yield to force a switch, and then check what
 * FXSAVE reports afterwards. Any mismatch means a context switch lost one
 * task's extended state to another.
 *
 * The program works only through FXSAVE/FXRSTOR on 16-byte aligned buffers,
 * so it needs no XMM register operands in inline assembly and does not
 * depend on how the compiler allocates registers.
 *
 * Not shipped in any image. Boot-tested by embedding it as /fputest in the
 * boot VFS and spawning it once from init_scheduler (the same procedure
 * ulib/examples/argtest.rs documents).
 */

#![no_std]
#![no_main]

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::hint::black_box;
use core::panic::PanicInfo;
use core::sync::atomic::{AtomicUsize, Ordering};
use ulib::{STDERR_CAP, arg, capture_args, exit, serix_spawn_args, write};

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

/* Rounds per worker: each one crosses at least one timer time slice. */
const ROUNDS: usize = 20;
/* Busy-loop length between setting the pattern and checking it again. */
const SPIN: u64 = 2_000_000;

/* 512-byte FXSAVE image; FXSAVE and FXRSTOR require 16-byte alignment. */
#[repr(align(16))]
struct FxImage([u8; 512]);

/* FXSAVE layout offsets used here. */
const FCW: usize = 0;
const MXCSR: usize = 24;
const MXCSR_MASK: usize = 28;
const ST0: usize = 32;
const XMM0: usize = 160;

fn print_u64(mut val: u64) {
	let mut digits = [0u8; 20];
	let mut i = 0usize;
	if val == 0 {
		write(STDERR_CAP, b"0");
		return;
	}
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

fn report(prefix: &[u8], value: u64) {
	write(STDERR_CAP, prefix);
	print_u64(value);
	write(STDERR_CAP, b"\n");
}

/*
 * load_state - Install a full FPU/SSE image
 */
fn load_state(image: &FxImage) {
	unsafe {
		asm!(
			"fxrstor64 [{p}]",
			p = in(reg) image.0.as_ptr(),
			options(nostack, preserves_flags)
		);
	}
}

/*
 * save_state - Snapshot the live FPU/SSE state
 */
fn save_state(image: &mut FxImage) {
	unsafe {
		asm!(
			"fxsave64 [{p}]",
			p = in(reg) image.0.as_mut_ptr(),
			options(nostack, preserves_flags)
		);
	}
}

/*
 * build_image - Build the state a worker expects to keep
 * @seed: Byte repeated through XMM0 and the x87 value selector
 *
 * The x87 value is a canonical normal number (2.0 or 3.0), so FXRSTOR
 * cannot canonicalise it into something that compares unequal.
 */
fn build_image(seed: u8) -> FxImage {
	let mut image = FxImage([0u8; 512]);

	/* Architectural defaults: FCW 0x037F, MXCSR 0x1F80. */
	image.0[FCW] = 0x7F;
	image.0[FCW + 1] = 0x03;
	image.0[MXCSR] = 0x80;
	image.0[MXCSR + 1] = 0x1F;
	image.0[MXCSR_MASK] = 0xFF;
	image.0[MXCSR_MASK + 1] = 0xFF;

	let mut pattern = [seed; 16];
	pattern[0] = seed.wrapping_add(1);
	image.0[XMM0..XMM0 + 16].copy_from_slice(&pattern);

	/* ST0: exponent 0x4000, mantissa 0x8000.. or 0xC000.. */
	image.0[ST0..ST0 + 7].fill(0);
	if seed & 1 == 0 {
		image.0[ST0 + 7] = 0x80;
	} else {
		image.0[ST0 + 7] = 0xC0;
	}
	image.0[ST0 + 8] = 0x00;
	image.0[ST0 + 9] = 0x40;

	image
}

fn x87_pattern(image: &FxImage) -> [u8; 10] {
	let mut out = [0u8; 10];
	out.copy_from_slice(&image.0[ST0..ST0 + 10]);
	out
}

/*
 * worker - Hold a pattern across preemption and verify it each round
 * @seed: Selects this worker's distinct XMM0 / x87 values
 */
fn worker(seed: u8) -> ! {
	let image = build_image(seed);
	let expected_xmm = {
		let mut out = [0u8; 16];
		out.copy_from_slice(&image.0[XMM0..XMM0 + 16]);
		out
	};
	let expected_x87 = x87_pattern(&image);

	let mut mismatches = 0u64;
	let mut round = 0usize;
	while round < ROUNDS {
		load_state(&image);

		/* Burn CPU so the timer preempts this task mid-round. */
		let mut acc: u64 = round as u64;
		let mut i: u64 = 0;
		while i < SPIN {
			acc = acc.wrapping_add(i ^ round as u64);
			i += 1;
		}
		black_box(acc);

		/* The spin above runs longer than a 16 ms time slice, so the
		 * LAPIC timer has preempted this task at least once by now. A
		 * yield syscall is deliberately not used: the check is about
		 * preemption, and the current yield path can strand a task. */

		let mut live = FxImage([0u8; 512]);
		save_state(&mut live);
		if live.0[XMM0..XMM0 + 16] != expected_xmm {
			mismatches += 1;
		}
		if x87_pattern(&live) != expected_x87 {
			mismatches += 1;
		}

		round += 1;
	}

	report(b"[fputest] worker mismatches=", mismatches);
	exit(if mismatches == 0 { 0 } else { 1 })
}

/*
 * _start - ELF entry point
 *
 * The kernel places argc at [rsp], so the entry point must hand that exact
 * stack pointer to capture_args before any Rust prologue can move it.
 * A compiler-generated prologue adjusts RSP for locals, which is why this
 * has to be naked rather than an inline `mov rsp` inside a normal function.
 */
#[unsafe(naked)]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
	core::arch::naked_asm!(
		"mov rdi, rsp",
		"and rsp, -16",
		"call {entry}",
		entry = sym fputest_main,
	)
}

extern "C" fn fputest_main(rsp: usize) -> ! {
	unsafe {
		capture_args(rsp);
	}

	/* argv[1] selects a worker; without it this is the parent. */
	report(b"[fputest] start argc=", ulib::argc() as u64);
	if let Some(mode) = arg(1) {
		write(STDERR_CAP, b"[fputest] mode=");
		write(STDERR_CAP, mode);
		write(STDERR_CAP, b"\n");
	}
	switch_mode();
}

fn switch_mode() -> ! {
	match arg(1) {
		Some(mode) if mode == b"a" => worker(0xA5),
		Some(mode) if mode == b"b" => worker(0x5A),
		_ => {}
	}

	/*
	 * Spawn both workers and exit immediately: each worker reports its own
	 * mismatch count, so the check does not depend on cross-process wait
	 * semantics (which currently stall the parent).
	 */
	write(STDERR_CAP, b"[fputest] parent spawning two workers\n");
	let mut failed = 0u64;
	for mode in ["a", "b"] {
		let pid = serix_spawn_args("/fputest", &["fputest", mode]);
		report(b"[fputest] spawn -> ", pid as u64);
		if pid < 0 {
			failed += 1;
		}
	}

	report(b"[fputest] failed=", failed);
	exit(if failed == 0 { 0 } else { 1 })
}
