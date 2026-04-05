/*
 * kshell.rs - Kernel-space Interactive Shell
 *
 * A simple built-in TTY shell that runs as a Ring-0 kernel task.
 * Bypasses all userspace machinery (ELF loader, syscalls, page tables).
 *
 * Input: PS/2 keyboard (interrupt-driven) OR COM1 serial (polled).
 *        Whichever arrives first wins — works on bare metal (PS/2) and
 *        in QEMU -serial stdio (serial).
 * Output: COM1 serial + framebuffer console.
 *
 * Spawned by spawn_kshell() which allocates a kernel stack and enqueues
 * the task before the timer starts.
 */

extern crate alloc;

use hal::{serial_print, serial_println};

const LINE_MAX: usize = 256;

/* kshell_task - Entry point for the kernel shell task
 *
 * Must be `unsafe extern "C" fn() -> !` to satisfy TaskCB::new.
 * Called via context_switch ret on the task's first time slice.
 */
pub unsafe extern "C" fn kshell_task() -> ! {
	serial_println!();
	serial_println!("=== Serix Kernel Shell ===");
	serial_println!("Type 'help' for commands.");
	serial_println!();

	let mut buf = [0u8; LINE_MAX];
	let mut len: usize = 0;

	serial_print!("ksh> ");

	loop {
		/* Block until a byte arrives from serial or PS/2 */
		let byte = read_byte();

		match byte {
			b'\r' | b'\n' => {
				serial_println!();
				let line = core::str::from_utf8(&buf[..len]).unwrap_or("");
				run_command(line.trim());
				len = 0;
				serial_print!("ksh> ");
			}
			/* Backspace / DEL */
			0x08 | 0x7F => {
				if len > 0 {
					len -= 1;
					serial_print!("\x08 \x08");
				}
			}
			/* Printable ASCII */
			b if b >= 0x20 && b < 0x7F => {
				if len < LINE_MAX - 1 {
					buf[len] = b;
					len += 1;
					/* Echo back */
					let s = core::str::from_utf8(core::slice::from_ref(&b)).unwrap_or("?");
					serial_print!("{}", s);
				}
			}
			_ => {}
		}
	}
}

/* read_byte - Block until input arrives from serial or PS/2
 *
 * Enables interrupts in the spin loop so the LAPIC timer can preempt
 * us and run other tasks while we wait. Accepts input from either
 * source so the shell works on bare metal (PS/2 only) and in QEMU
 * with -serial stdio.
 */
fn read_byte() -> u8 {
	loop {
		x86_64::instructions::interrupts::enable();

		if let Some(b) = hal::serial::serial_read_byte() {
			x86_64::instructions::interrupts::disable();
			return b;
		}
		if let Some(b) = keyboard::pop_key() {
			x86_64::instructions::interrupts::disable();
			return b;
		}

		core::hint::spin_loop();
	}
}

/* run_command - Dispatch a trimmed command line to a builtin handler */
fn run_command(line: &str) {
	if line.is_empty() {
		return;
	}

	let mut parts = line.splitn(2, ' ');
	let cmd  = parts.next().unwrap_or("");
	let args = parts.next().unwrap_or("").trim();

	match cmd {
		"help" => {
			serial_println!("Available commands:");
			serial_println!("  help              - show this message");
			serial_println!("  echo <text>       - print text");
			serial_println!("  halt              - stop the CPU");
			serial_println!("  reboot            - triple-fault reboot");
		}
		"echo" => {
			serial_println!("{}", args);
			graphics::fb_println!("{}", args);
		}
		"halt" => {
			serial_println!("Halting system.");
			x86_64::instructions::interrupts::disable();
			loop {
				x86_64::instructions::hlt();
			}
		}
		"reboot" => {
			serial_println!("Rebooting...");
			unsafe {
				/* Triple-fault by loading a zero-length IDT then dividing by zero */
				core::arch::asm!(
					"sub rsp, 10",
					"mov word ptr [rsp], 0",    /* limit = 0 */
					"mov qword ptr [rsp+2], 0", /* base = 0  */
					"lidt [rsp]",
					"int 0",
					options(nostack, noreturn)
				);
			}
		}
		_ => {
			serial_println!("ksh: {}: command not found", cmd);
		}
	}
}

/* spawn_kshell - Allocate a kernel stack and enqueue the kshell task
 *
 * Called from _start after kstack region and scheduler are initialised.
 * Returns the new task's numeric ID on success.
 */
pub fn spawn_kshell() -> Result<u64, &'static str> {
	let kstack = memory::kstack::alloc_kernel_stack(64 * 1024)
		.ok_or("kshell: OOM allocating stack")?;

	let id = task::TaskId::new();
	let id_val = id.0;

	let mut ctx = task::CPUContext::default();
	ctx.rsp    = kstack.as_u64();
	ctx.rip    = kshell_task as u64;
	ctx.cr3    = 0;     /* 0 = keep kernel CR3 (no page-table switch) */
	ctx.cs     = 0x08;
	ctx.ss     = 0x10;
	ctx.rflags = 0x202;

	let tcb = task::TaskCB {
		id,
		state:             task::TaskState::Ready,
		sched_class:       task::SchedClass::Fair(120),
		context:           ctx,
		kstack,
		ustack:            None,
		name:              "kshell",
		parent_id:         0,
		exit_status:       None,
		pml4_frame:        None,
		children:          alloc::vec::Vec::new(),
		waiting_for_child: false,
	};

	task::scheduler::enqueue_task(alloc::sync::Arc::new(spin::Mutex::new(tcb)));
	Ok(id_val)
}
