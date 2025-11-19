use core::arch::naked_asm;
use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

// Syscall Numbers
pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 60;
pub const SYS_YIELD: u64 = 24;

pub fn init_syscalls() {
	// 1. Enable System Call Extensions (SCE) in EFER
	unsafe {
		let mut efer = Efer::read();
		efer |= EferFlags::SYSTEM_CALL_EXTENSIONS;
		Efer::write(efer);
	}

	// 2. Setup LSTAR (Target RIP for syscall)
	let syscall_addr = syscall_entry as usize as u64;
	LStar::write(VirtAddr::new(syscall_addr));

	// 3. Setup STAR (Segment Selectors)
	// x86_64 crate expectation:
	// Arg 0 (user_code_selector) must be Arg 1 (user_data_selector) + 8
	// STAR gets Arg 1.
	//
	// We pass:
	// Arg 1 (Base) = Index 3 (0x18)
	// Arg 0 (Target) = Index 4 (0x20)
	//
	// Hardware behavior (SYSRET):
	// SS = STAR + 8 = Index 3 + 1 = Index 4 (User Data) -> Correct
	// CS = STAR + 16 = Index 3 + 2 = Index 5 (User Code) -> Correct
	Star::write(
		x86_64::structures::gdt::SegmentSelector::new(4, x86_64::PrivilegeLevel::Ring3), // Arg0: "User Code" (actually User Data for validation)
		x86_64::structures::gdt::SegmentSelector::new(3, x86_64::PrivilegeLevel::Ring3), // Arg1: "User Data" (Base)
		x86_64::structures::gdt::SegmentSelector::new(1, x86_64::PrivilegeLevel::Ring0), // Kernel Code
		x86_64::structures::gdt::SegmentSelector::new(2, x86_64::PrivilegeLevel::Ring0), // Kernel Data
	)
	.unwrap();

	// 4. Setup SFMASK (RFlags mask)
	// Mask interrupts and traps on entry
	SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
	naked_asm!(
		"swapgs",               // Swap to kernel GS
		"mov gs:[16], rsp",     // Save user stack in scratch
		"mov rsp, gs:[8]",      // Load kernel stack

		"push r11",             // Save user RFLAGS
		"push rcx",             // Save user RIP
		"push rbp",             // Save callee-saved regs
		"push rbx",
		"push r12",
		"push r13",
		"push r14",
		"push r15",

		// ABI Mapping:
		// RAX -> RDI (nr)
		// RDI -> RSI (arg1)
		// RSI -> RDX (arg2)
		// RDX -> RCX (arg3)

		"mov rcx, rdx",         // Arg3
		"mov rdx, rsi",         // Arg2
		"mov rsi, rdi",         // Arg1
		"mov rdi, rax",         // Nr

		"call {syscall_handler}",

		"pop r15",
		"pop r14",
		"pop r13",
		"pop r12",
		"pop rbx",
		"pop rbp",
		"pop rcx",              // Restore user RIP
		"pop r11",              // Restore user RFLAGS

		"mov rsp, gs:[16]",     // Restore user stack
		"swapgs",               // Restore user GS
		"sysretq",
		syscall_handler = sym syscall_dispatcher,
	);
}

#[unsafe(no_mangle)]
extern "C" fn syscall_dispatcher(nr: u64, arg1: u64, arg2: u64, arg3: u64) {
	match nr {
		SYS_WRITE => {
			// Arg1: fd, Arg2: ptr, Arg3: len
			if arg1 == 1 {
				// stdout
				let ptr = arg2 as *const u8;
				let len = arg3 as usize;
				// Safety: We are assuming valid UTF-8 and pointer for this basic impl
				let s = unsafe {
					core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len))
				};
				hal::serial_print!("{}", s);
			}
		}
		SYS_EXIT => {
			hal::serial_println!("Process exited with status {}", arg1);
			task::preempt_executor();
		}
		SYS_YIELD => {
			task::preempt_executor();
		}
		_ => {
			hal::serial_println!("Unknown syscall: {}", nr);
		}
	}
}
