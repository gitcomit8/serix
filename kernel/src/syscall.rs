/*
 * System Call Handler
 *
 * Implements fast system calls using SYSCALL/SYSRET instructions.
 * Handles system call entry, register marshalling, and return to userspace.
 */

use core::arch::naked_asm;
use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

/* System call numbers */
pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 60;
pub const SYS_YIELD: u64 = 24;

/*
 * init_syscalls - Initialize system call support
 *
 * Configures MSRs for SYSCALL/SYSRET instructions:
 * - Enables System Call Extensions in EFER
 * - Sets LSTAR to point to syscall entry handler
 * - Configures STAR with kernel and user segment selectors
 * - Sets SFMASK to mask interrupts during syscall entry
 */
pub fn init_syscalls() {
	/* Enable System Call Extensions (SCE) in EFER */
	unsafe {
		let mut efer = Efer::read();
		efer |= EferFlags::SYSTEM_CALL_EXTENSIONS;
		Efer::write(efer);
	}

	/* Setup LSTAR (Target RIP for syscall) */
	let syscall_addr = syscall_entry as usize as u64;
	LStar::write(VirtAddr::new(syscall_addr));

	/*
	 * Setup STAR (Segment Selectors)
	 * x86_64 crate expectation:
	 * Arg 0 (user_code_selector) must be Arg 1 (user_data_selector) + 8
	 * STAR gets Arg 1.
	 *
	 * We pass:
	 * Arg 1 (Base) = Index 3 (0x18)
	 * Arg 0 (Target) = Index 4 (0x20)
	 *
	 * Hardware behavior (SYSRET):
	 * SS = STAR + 8 = Index 3 + 1 = Index 4 (User Data) -> Correct
	 * CS = STAR + 16 = Index 3 + 2 = Index 5 (User Code) -> Correct
	 */
	Star::write(
		x86_64::structures::gdt::SegmentSelector::new(4, x86_64::PrivilegeLevel::Ring3),
		x86_64::structures::gdt::SegmentSelector::new(3, x86_64::PrivilegeLevel::Ring3),
		x86_64::structures::gdt::SegmentSelector::new(1, x86_64::PrivilegeLevel::Ring0),
		x86_64::structures::gdt::SegmentSelector::new(2, x86_64::PrivilegeLevel::Ring0),
	)
	.unwrap();

	/* Setup SFMASK to mask interrupts and traps on entry */
	SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);
}

/*
 * syscall_entry - Low-level syscall entry point
 *
 * Naked assembly function that handles the transition from user to kernel mode.
 * Saves user context, switches to kernel stack, and calls the dispatcher.
 */
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
	naked_asm!(
		/* Swap to kernel GS and save/load stacks */
		"swapgs",
		"mov gs:[16], rsp",
		"mov rsp, gs:[8]",

		/* Save user RFLAGS and RIP (saved by SYSCALL instruction) */
		"push r11",
		"push rcx",

		/* Save callee-saved registers */
		"push rbp",
		"push rbx",
		"push r12",
		"push r13",
		"push r14",
		"push r15",

		/*
		 * ABI Mapping from Linux syscall ABI to System V ABI:
		 * RAX (syscall nr) -> RDI (arg0)
		 * RDI (arg1) -> RSI (arg1)
		 * RSI (arg2) -> RDX (arg2)
		 * RDX (arg3) -> RCX (arg3)
		 */
		"mov rcx, rdx",
		"mov rdx, rsi",
		"mov rsi, rdi",
		"mov rdi, rax",

		/* Call the syscall dispatcher */
		"call {syscall_handler}",

		/* Restore callee-saved registers */
		"pop r15",
		"pop r14",
		"pop r13",
		"pop r12",
		"pop rbx",
		"pop rbp",

		/* Restore user RIP and RFLAGS for SYSRET */
		"pop rcx",
		"pop r11",

		/* Restore user stack and GS, then return to userspace */
		"mov rsp, gs:[16]",
		"swapgs",
		"sysretq",
		syscall_handler = sym syscall_dispatcher,
	);
}

/*
 * syscall_dispatcher - High-level syscall handler
 * @nr: System call number
 * @arg1: First argument
 * @arg2: Second argument
 * @arg3: Third argument
 *
 * Dispatches system calls to appropriate handlers based on the syscall number.
 */
#[unsafe(no_mangle)]
extern "C" fn syscall_dispatcher(nr: u64, arg1: u64, arg2: u64, arg3: u64) {
	match nr {
		SYS_WRITE => {
			/* Write system call: fd, buffer pointer, length */
			if arg1 == 1 {
				/* stdout */
				let ptr = arg2 as *const u8;
				let len = arg3 as usize;
				let s = unsafe {
					core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len))
				};
				hal::serial_print!("{}", s);
			}
		}
		SYS_EXIT => {
			/* Exit system call: terminate current task */
			hal::serial_println!("Process exited with status {}", arg1);
			task::preempt_executor();
		}
		SYS_YIELD => {
			/* Yield system call: voluntarily give up CPU */
			task::preempt_executor();
		}
		_ => {
			hal::serial_println!("Unknown syscall: {}", nr);
		}
	}
}
