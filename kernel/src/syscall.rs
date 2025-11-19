use core::arch::{asm, naked_asm};
use x86_64::{
	registers::{
		model_specific::{Efer, EferFlags, LStar, SFMask, Star},
		rflags::RFlags,
	},
	VirtAddr,
};

pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 60;
pub const SYS_YIELD: u64 = 24;

pub fn init_syscalls() {
	//Enable SCE in EFER
	unsafe {
		let mut efer = Efer::read();
		efer |= EferFlags::SYSTEM_CALL_EXTENSIONS;
		Efer::write(efer);
	}

	//Setup LSTAR
	let syscall_addr = syscall_entry as usize as u64;
	LStar::write(VirtAddr::new(syscall_addr));

	//Setup STAR
	//Kernel: Code=0x08, Data=0x10
	//User:   Code=0x20, Data=0x18
	Star::write(
		x86_64::structures::gdt::SegmentSelector::new(0x08, x86_64::PrivilegeLevel::Ring0),
		x86_64::structures::gdt::SegmentSelector::new(0x08, x86_64::PrivilegeLevel::Ring0),
		x86_64::structures::gdt::SegmentSelector::new(0x180, x86_64::PrivilegeLevel::Ring3),
		x86_64::structures::gdt::SegmentSelector::new(0x200, x86_64::PrivilegeLevel::Ring3),
	)
	.unwrap();

	//Setup SFMASK
	SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::TRAP_FLAG);
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
	naked_asm!("swapgs",
	"mov gs:[16], rsp",
	"mov rsp, gs:[8]",

	"push r11",
	"push rcx",         // Save callee-saved regs
	"push rbp",
	"push rbx",
	"push r12",
	"push r13",
	"push r14",
	"push r15",

	/*
	ABI Mapping:
	RAX -> RDI (nr)
	RDI -> RSI (arg1)
	RSI -> RDX (arg2)
	RDX -> RCX (arg3)
	 */

	"mov rcx, rdx",     // Arg3
	"mov rdx, rsi",     // Arg2
	"mov rsi, rdi",     // Arg1
	"mov rdi, rax",     // Nr

	"call {syscall_handler}",

	"pop r15",
	"pop r14",
	"pop r13",
	"pop r12",
	"pop rbx",
	"pop rbp",
	"pop rcx",          // Restore user RIP
	"pop r11",          // Restore user RFlags

	"mov rsp, gs:[16]", // Restore user stack
	"swapgs",           // Restore user GS
	"sysretq",
	syscall_handler = sym syscall_dispatcher
	);
}

#[unsafe(no_mangle)]
extern "C" fn syscall_dispatcher(nr: u64, arg1: u64, arg2: u64, arg3: u64) {
	match nr {
		SYS_WRITE => {
			if arg1 == 1 {
				let ptr = arg2 as *const u8;
				let len = arg3 as usize;
				//Safety: Assume valid UTF-8 and pointer for basic impl
				let s = unsafe {
					core::str::from_utf8_unchecked(core::slice::from_raw_parts(ptr, len))
				};
				hal::serial_println!("{}", s);
			}
		}
		SYS_EXIT => {
			hal::serial_println!("Process exited with code {}", arg1);
			task::preempt_executor();
		}
		SYS_YIELD => task::preempt_executor(),
		_ => {
			hal::serial_println!("Unknown syscall {}", nr);
		}
	}
}
