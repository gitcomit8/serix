/*
 * System Call Handler
 *
 * Implements fast system calls using SYSCALL/SYSRET instructions.
 * Handles system call entry, register marshaling, and return to userspace.
 */

use core::arch::naked_asm;
use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
use x86_64::VirtAddr;

/* System call numbers */
pub const SYS_WRITE: u64 = 1;
pub const SYS_EXIT: u64 = 60;
pub const SYS_YIELD: u64 = 24;
pub const SYS_SEND: u64 = 20;
pub const SYS_RECV: u64 = 21;

/* Error codes (negative errno values represented as u64) */
pub const ERRNO_EBADF: u64 = u64::MAX - 8; /* Bad file descriptor (errno 9) */
pub const ERRNO_EFAULT: u64 = u64::MAX - 13; /* Bad address (errno 14) */
pub const ERRNO_EINVAL: u64 = u64::MAX - 21; /* Invalid argument (errno 22) */

/* Userspace memory validation constants */
const USER_SPACE_START: u64 = 0x0000_0000_0000_0000;
const USER_SPACE_END: u64 = 0x0000_8000_0000_0000; /* 128 TiB - typical userspace limit */

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
 * is_user_accessible - Validate userspace pointer range
 * @ptr: Pointer to validate
 * @len: Length of memory region
 *
 * Returns true if the entire memory range [ptr, ptr+len) is in valid userspace.
 */
#[inline]
fn is_user_accessible(ptr: *const u8, len: usize) -> bool {
	let addr = ptr as u64;
	let end_addr = addr.saturating_add(len as u64);

	/* Check for overflow and valid userspace range */
	addr >= USER_SPACE_START && end_addr <= USER_SPACE_END && end_addr > addr && !ptr.is_null()
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
		"mov gs:[16], rsp",      /* Save user stack pointer */
		"mov rsp, gs:[8]",       /* Load kernel stack pointer */

		/* Align stack to 16 bytes as required by System V ABI */
		"and rsp, ~0xF",

		/* Save user RFLAGS and RIP (saved by SYSCALL instruction) */
		"push r11",              /* User RFLAGS */
		"push rcx",              /* User RIP */

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
		 * R10 (arg3) -> RCX (arg3)
		 * R8  (arg4) -> R8  (arg4)
		 * R9  (arg5) -> R9  (arg5)
		 */
		"mov r9, r8",            /* arg5 */
		"mov r8, r10",           /* arg4 (was saved in R10 by userspace) */
		"mov rcx, rdx",          /* arg3 */
		"mov rdx, rsi",          /* arg2 */
		"mov rsi, rdi",          /* arg1 */
		"mov rdi, rax",          /* syscall number */

		/* Call the syscall dispatcher - return value comes back in RAX */
		"call {syscall_handler}",

		/* RAX now contains the return value - preserve it */

		/* Restore callee-saved registers */
		"pop r15",
		"pop r14",
		"pop r13",
		"pop r12",
		"pop rbx",
		"pop rbp",

		/* Restore user RIP and RFLAGS for SYSRET */
		"pop rcx",               /* User RIP */
		"pop r11",               /* User RFLAGS */

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
 * @arg4: Fourth argument (optional, for future use)
 * @arg5: Fifth argument (optional, for future use)
 *
 * Dispatches system calls to appropriate handlers based on the syscall number.
 * Returns the syscall result in RAX (0 or positive on success, negative errno on error).
 */
#[unsafe(no_mangle)]
extern "C" fn syscall_dispatcher(
	nr: u64,
	arg1: u64,
	arg2: u64,
	arg3: u64,
	arg4: u64,
	_arg5: u64,
) -> u64 {
	match nr {
		SYS_WRITE => {
			/* Write system call: fd, buffer pointer, length */
			if arg1 != 1 {
				/* Only stdout (fd 1) is supported for now */
				return ERRNO_EBADF;
			}

			let ptr = arg2 as *const u8;
			let len = arg3 as usize;

			/* Validate pointer is in userspace range */
			if !is_user_accessible(ptr, len) {
				hal::serial_println!("[SYSCALL] SYS_WRITE: Invalid pointer 0x{:x}", arg2);
				return ERRNO_EFAULT;
			}

			/* Safely create slice from validated pointer */
			let slice = unsafe { core::slice::from_raw_parts(ptr, len) };

			/* Validate UTF-8 encoding */
			match core::str::from_utf8(slice) {
				Ok(s) => {
					hal::serial_print!("{}", s);
					len as u64 /* Return bytes written */
				}
				Err(_) => {
					hal::serial_println!("[SYSCALL] SYS_WRITE: Invalid UTF-8 data");
					ERRNO_EINVAL
				}
			}
		}

		SYS_EXIT => {
			/* Exit system call: terminate current task */
			hal::serial_println!("[SYSCALL] Process exited with status {}", arg1);
			loop {
				hal::cpu::halt();
			}
		}

		SYS_YIELD => {
			/* Yield system call: voluntarily give up CPU */
			task::preempt_executor();
			0 /* Success */
		}
		SYS_SEND => {
			/* * Send IPC Message
			 * arg1: Target Port ID
			 * arg2: Message ID/Type
			 * arg3: Pointer to data buffer (userspace)
			 * arg4: Data length
			 */
			let port_id = arg1;
			let msg_type = arg2;
			let ptr = arg3 as *const u8;
			let len = arg4 as usize;

			if len > ipc::MAX_MSG_SIZE {
				return ERRNO_EINVAL;
			}

			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}

			// Copy data from user
			let mut data = [0u8; ipc::MAX_MSG_SIZE];
			unsafe {
				core::ptr::copy_nonoverlapping(ptr, data.as_mut_ptr(), len);
			}

			let msg = ipc::Message {
				sender_id: 0, // TODO: Get current task ID
				id: msg_type,
				len: len as u64,
				data,
			};

			if let Some(port) = ipc::IPC_GLOBAL.get_port(port_id) {
				if port.send(msg) {
					0
				} else {
					// Queue full (EAGAIN)
					u64::MAX - 11
				}
			} else {
				// Port not found (ENOENT)
				u64::MAX - 2
			}
		}

		SYS_RECV => {
			/*
			 * Receive IPC Message
			 * arg1: Local Port ID
			 * arg2: Pointer to buffer to write data
			 * Returns: Message Type (id) in RAX, Length in RDX (needs custom return handling)
			 * For simplicity now: Returns 0 on success, fills buffer.
			 */
			let port_id = arg1;
			let out_ptr = arg2 as *mut u8;

			if let Some(port) = ipc::IPC_GLOBAL.get_port(port_id) {
				if let Some(msg) = port.receive() {
					// Validate output buffer
					let len = msg.len as usize;
					if !is_user_accessible(out_ptr, len) {
						return ERRNO_EFAULT;
					}

					unsafe {
						core::ptr::copy_nonoverlapping(msg.data.as_ptr(), out_ptr, len);
					}
					// Return Message ID (User needs to know what they got)
					msg.id
				} else {
					// No message (EAGAIN)
					u64::MAX - 11
				}
			} else {
				ERRNO_EINVAL
			}
		}
		_ => {
			/* Unknown system call */
			hal::serial_println!("[SYSCALL] Unknown syscall: {}", nr);
			ERRNO_EINVAL
		}
	}
}
