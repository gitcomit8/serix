/*
 * System Call Handler
 *
 * Implements fast system calls using SYSCALL/SYSRET instructions.
 * Handles system call entry, register marshaling, and return to userspace.
 */

use core::arch::naked_asm;
use hal::serial_println;
use x86_64::VirtAddr;
use x86_64::registers::model_specific::{Efer, EferFlags, LStar, SFMask, Star};
use x86_64::registers::rflags::RFlags;
/* System call numbers */
pub const SYS_READ: u64 = 0;
pub const SYS_WRITE: u64 = 1;
pub const SYS_OPEN: u64 = 2;
pub const SYS_CLOSE: u64 = 3;
pub const SYS_SEEK: u64 = 8;
pub const SYS_SEND: u64 = 20;
pub const SYS_RECV: u64 = 21;
pub const SYS_RECV_BLOCK: u64 = 22;
pub const SYS_YIELD: u64 = 24;
pub const SYS_EXIT: u64 = 60;
pub const SYS_WAIT4: u64 = 61;
pub const SYS_MKDIR: u64 = 83;
pub const SYS_UNLINK: u64 = 87;
pub const SYS_GETPID: u64 = 39;
pub const SYS_GETPPID: u64 = 110;
pub const SYS_SPAWN: u64 = 400;

/* Error codes (negative errno values represented as u64) */
pub const ERRNO_EBADF: u64 = u64::MAX - 8;  /* Bad file descriptor (errno 9) */
pub const ERRNO_ECHILD: u64 = u64::MAX - 9;  /* No child processes (errno 10) */
pub const ERRNO_EAGAIN: u64 = u64::MAX - 11; /* Resource temporarily unavailable */
pub const ERRNO_ENOMEM: u64 = u64::MAX - 11; /* Out of memory (errno 12) */
pub const ERRNO_EFAULT: u64 = u64::MAX - 13; /* Bad address (errno 14) */
pub const ERRNO_ENOENT: u64 = u64::MAX - 2;  /* No such file or entry */
pub const ERRNO_EINVAL: u64 = u64::MAX - 21; /* Invalid argument (errno 22) */
pub const ERRNO_ENOTDIR: u64 = u64::MAX - 19; /* Not a directory (errno 20) */
pub const ERRNO_EPIPE: u64 = u64::MAX - 31;  /* Broken pipe (errno 32) */

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
/*
 * syscall_entry - Low-level syscall entry point
 *
 * Naked assembly function that handles the transition from user to kernel mode.
 * Saves ALL user context, switches to kernel stack, calls dispatcher,
 * and restores context exactly as it was (except RAX).
 */
#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
	naked_asm!(
		/* 1. Swap to kernel GS and save user stack */
		"swapgs",
		"mov gs:[16], rsp",      /* Save user stack pointer */
		"mov rsp, gs:[8]",       /* Load kernel stack pointer */

		/* 2. Align stack to 16 bytes */
		"and rsp, ~0xF",

		/* 3. Save User Context (The "Trap Frame") */
		/* We must save registers that we clobber or that the ABI expects preserved */
		"push r11",              /* User RFLAGS (clobbered by syscall) */
		"push rcx",              /* User RIP (clobbered by syscall) */

		/* Save arguments & callee-saved registers */
		"push r9",
		"push r8",
		"push r10",
		"push rdx",
		"push rsi",
		"push rdi",
		"push rax",              /* Save original RAX (syscall nr) just in case */

		"push rbp",
		"push rbx",
		"push r12",
		"push r13",
		"push r14",
		"push r15",

		/* 4. Prepare Arguments for syscall_dispatcher (System V ABI) */
		/*
		 * Kernel Function: fn(nr, arg1, arg2, arg3, arg4, arg5)
		 * Mapping:
		 * RDI <- RAX (nr)
		 * RSI <- RDI (arg1)
		 * RDX <- RSI (arg2)
		 * RCX <- RDX (arg3)
		 * R8  <- R10 (arg4 - syscall puts it here)
		 * R9  <- R8  (arg5)
		 */
		"mov r9, r8",            /* arg5 */
		"mov r8, r10",           /* arg4 */
		"mov rcx, rdx",          /* arg3 */
		"mov rdx, rsi",          /* arg2 */
		"mov rsi, rdi",          /* arg1 */
		"mov rdi, rax",          /* syscall number */

		/* 5. Call Dispatcher */
		"call {syscall_handler}",

		/* RAX now holds the return value. We must NOT overwrite it when restoring. */

		/* 6. Restore Context */
		"pop r15",
		"pop r14",
		"pop r13",
		"pop r12",
		"pop rbx",
		"pop rbp",

		/* Skip RAX on stack (we want to keep the new return value in real RAX) */
		"add rsp, 8",

		"pop rdi",
		"pop rsi",
		"pop rdx",
		"pop r10",
		"pop r8",
		"pop r9",

		"pop rcx",               /* User RIP */
		"pop r11",               /* User RFLAGS */

		/* 7. Return to Userspace */
		"mov rsp, gs:[16]",      /* Restore User Stack */
		"swapgs",                /* Restore User GS */
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
		SYS_READ => {
			/* Read system call: fd, buffer pointer, length */
			let fd = arg1;
			let ptr = arg2 as *mut u8;
			let len = arg3 as usize;

			if len == 0 {
				return 0;
			}
			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}

			let task_id = task::scheduler::current_task_id();
			if let Some(file) = crate::fd::get(task_id, fd) {
				let mut off = file.offset.lock();
				let buf = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
				let n = file.inode.read(*off, buf);
				*off += n;
				n as u64
			} else {
				ERRNO_EBADF
			}
		}
		SYS_WRITE => {
			/* Write system call: fd, buffer pointer, length */
			let fd = arg1;
			let ptr = arg2 as *const u8;
			let len = arg3 as usize;

			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}

			let task_id = task::scheduler::current_task_id();
			if let Some(file) = crate::fd::get(task_id, fd) {
				let mut off = file.offset.lock();
				let buf = unsafe { core::slice::from_raw_parts(ptr, len) };
				let n = file.inode.write(*off, buf);
				*off += n;
				n as u64
			} else {
				ERRNO_EBADF
			}
		}

		SYS_EXIT => {
			/*
			 * Exit — terminate the calling task.
			 * Marks the task Zombie, closes all fds, wakes any waiting parent,
			 * then context-switches away. Never returns to the caller.
			 */
			let status = arg1 as i32;
			hal::serial_println!("[SYSCALL] SYS_EXIT status={}", status);

			x86_64::instructions::interrupts::without_interrupts(|| {
				let old_arc = match task::scheduler::take_current() {
					Some(t) => t,
					None => loop { hal::cpu::halt(); },
				};

				{
					let mut t = old_arc.lock();
					t.exit_status = Some(status);
					t.set_state(task::TaskState::Zombie);
				}

				let task_id = old_arc.lock().id.0;
				crate::fd::cleanup(task_id);

				/* Wake parent if it is blocked in SYS_WAIT4 */
				let parent_id = old_arc.lock().parent_id;
				if parent_id != 0 {
					if let Some(par) = task::scheduler::find_task_by_id(parent_id) {
						let waiting = par.lock().waiting_for_child;
						if waiting {
							par.lock().waiting_for_child = false;
							task::scheduler::wake_task(par);
						}
					}
				}

				task::scheduler::push_zombie(old_arc);

				/* Switch to next task */
				if let Some(new_arc) = task::scheduler::pick_next_task() {
					if let Some(hook) = task::SWITCH_HOOK.get() {
						let ks = new_arc.lock().kstack;
						if ks.as_u64() != 0 { hook(ks); }
					}
					let new_ctx = {
						let g = new_arc.lock();
						&g.context as *const task::CPUContext
					};
					/* Use a dummy old context (never resumed) */
					static mut DUMMY_CTX: task::CPUContext = task::CPUContext {
						rsp: 0, rbp: 0, rbx: 0, r12: 0, r13: 0,
						r14: 0, r15: 0, rip: 0, rflags: 0,
						cs: 0, ss: 0, fs: 0, gs: 0, ds: 0, es: 0,
						fs_base: 0, gs_base: 0, cr3: 0,
					};
					unsafe {
						task::context_switch::context_switch(
							core::ptr::addr_of_mut!(DUMMY_CTX),
							new_ctx,
						);
					}
				}
			});
			loop { hal::cpu::halt(); }
		}

		SYS_GETPID => task::scheduler::current_task_id(),

		SYS_GETPPID => {
			let id = task::scheduler::current_task_id();
			if let Some(arc) = task::scheduler::current_task_arc() {
				arc.lock().parent_id
			} else {
				0
			}
		}

		SYS_WAIT4 => {
			/*
			 * Wait for a child process to exit.
			 * arg1: pid (-1 = any child)
			 * arg2: pointer to i32 for exit status (may be null)
			 * arg3: options (WNOHANG = 1)
			 */
			let pid = arg1 as i64;
			let status_ptr = arg2 as *mut i32;
			let wnohang = arg3 & 1 != 0;

			let task_id = task::scheduler::current_task_id();

			/* Check if the requested child is actually our child */
			if pid > 0 {
				let is_child = task::scheduler::current_task_arc()
					.map(|a| a.lock().children.contains(&(pid as u64)))
					.unwrap_or(false);
				if !is_child {
					return ERRNO_ECHILD;
				}
			} else {
				/* pid == -1: check we have at least one child */
				let has_children = task::scheduler::current_task_arc()
					.map(|a| !a.lock().children.is_empty())
					.unwrap_or(false);
				if !has_children {
					return ERRNO_ECHILD;
				}
			}

			loop {
				if let Some(zombie) = task::scheduler::find_zombie_child(task_id, pid) {
					let (child_pid, exit_status) = {
						let t = zombie.lock();
						(t.id.0, t.exit_status.unwrap_or(0))
					};

					/* Remove from parent's children list */
					if let Some(par) = task::scheduler::current_task_arc() {
						par.lock().children.retain(|&id| id != child_pid);
					}

					/* Write status to userspace if pointer is valid */
					if !status_ptr.is_null()
						&& is_user_accessible(status_ptr as *const u8, 4)
					{
						unsafe { *status_ptr = (exit_status & 0xFF) << 8; }
					}

					return child_pid;
				}

				if wnohang {
					return 0;
				}

				/* No zombie yet — block until a child exits */
				x86_64::instructions::interrupts::without_interrupts(|| {
					if let Some(arc) = task::scheduler::current_task_arc() {
						arc.lock().waiting_for_child = true;
					}
					task::block_current_and_switch();
				});
			}
		}

		SYS_SPAWN => {
			/*
			 * Spawn a new user process from an ELF on the VFS.
			 * arg1: path pointer, arg2: path length
			 * Returns: child pid on success, errno on failure
			 */
			let ptr = arg1 as *const u8;
			let len = arg2 as usize;

			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}
			let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
			let path = match core::str::from_utf8(slice) {
				Ok(s) => s,
				Err(_) => return ERRNO_EINVAL,
			};

			let parent_id = task::scheduler::current_task_id();
			match crate::process::spawn_user_process(path, parent_id) {
				Ok(child_id) => child_id,
				Err(msg) => {
					hal::serial_println!("[SPAWN] failed: {}", msg);
					ERRNO_ENOENT
				}
			}
		}

		SYS_OPEN => {
			/*
			 * Open system call: path_ptr, path_len
			 * Returns: fd on success, ENOENT if path not found
			 */
			let ptr = arg1 as *const u8;
			let len = arg2 as usize;

			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}

			let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
			let path = match core::str::from_utf8(slice) {
				Ok(s) => s,
				Err(_) => return ERRNO_EINVAL,
			};

			let task_id = task::scheduler::current_task_id();
			match crate::fd::open(task_id, path) {
				Some(fd) => fd,
				None => ERRNO_ENOENT,
			}
		}

		SYS_CLOSE => {
			/*
			 * Close system call: fd
			 * Returns: 0 on success, EBADF if fd not found
			 */
			let fd = arg1;
			let task_id = task::scheduler::current_task_id();
			if crate::fd::close(task_id, fd) {
				0
			} else {
				ERRNO_EBADF
			}
		}

		SYS_SEEK => {
			/*
			 * Seek system call: fd, offset
			 * Returns: 0 on success, EBADF if fd not found
			 */
			let fd = arg1;
			let offset = arg2 as usize;
			let task_id = task::scheduler::current_task_id();
			if crate::fd::seek(task_id, fd, offset) {
				0
			} else {
				ERRNO_EBADF
			}
		}

		SYS_YIELD => {
			/* Yield system call: voluntarily give up CPU */
			task::preempt_executor();
			0 /* Success */
		}
		SYS_SEND => {
			/*
			 * Send IPC Message
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

			let mut data = [0u8; ipc::MAX_MSG_SIZE];
			unsafe {
				core::ptr::copy_nonoverlapping(ptr, data.as_mut_ptr(), len);
			}

			let msg = ipc::Message {
				sender_id: task::scheduler::current_task_id(),
				id: msg_type,
				len: len as u64,
				data,
			};

			if let Some(port) = ipc::IPC_GLOBAL.get_port(port_id) {
				x86_64::instructions::interrupts::without_interrupts(|| {
					if port.send(msg) { 0 } else { ERRNO_EAGAIN }
				})
			} else {
				ERRNO_ENOENT
			}
		}

		SYS_RECV => {
			/*
			 * Receive IPC Message (non-blocking)
			 * arg1: Local Port ID
			 * arg2: Pointer to buffer to write data
			 * Returns: Message ID in RAX, or EAGAIN if empty
			 */
			let port_id = arg1;
			let out_ptr = arg2 as *mut u8;

			if let Some(port) = ipc::IPC_GLOBAL.get_port(port_id) {
				if let Some(msg) = port.receive() {
					let len = msg.len as usize;
					if !is_user_accessible(out_ptr, len) {
						return ERRNO_EFAULT;
					}
					unsafe {
						core::ptr::copy_nonoverlapping(
							msg.data.as_ptr(), out_ptr, len,
						);
					}
					msg.id
				} else {
					ERRNO_EAGAIN
				}
			} else {
				ERRNO_ENOENT
			}
		}

		SYS_RECV_BLOCK => {
			/*
			 * Blocking Receive IPC Message
			 * arg1: Local Port ID
			 * arg2: Pointer to buffer to write message data
			 * Returns: Message ID in RAX
			 *
			 * Blocks the calling task until a message is available.
			 */
			let port_id = arg1;
			let out_ptr = arg2 as *mut u8;

			let port = match ipc::IPC_GLOBAL.get_port(port_id) {
				Some(p) => p,
				None => return ERRNO_ENOENT,
			};

			let msg = x86_64::instructions::interrupts::without_interrupts(|| {
				port.receive_blocking()
			});

			let len = msg.len as usize;
			if !is_user_accessible(out_ptr, len) {
				return ERRNO_EFAULT;
			}
			unsafe {
				core::ptr::copy_nonoverlapping(
					msg.data.as_ptr(), out_ptr, len,
				);
			}
			msg.id
		}
		SYS_MKDIR => {
			let ptr = arg1 as *const u8;
			let len = arg2 as usize;

			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}
			let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
			let path = match core::str::from_utf8(slice) {
				Ok(s) => s,
				Err(_) => return ERRNO_EINVAL,
			};

			let (parent_path, name) = match path.rfind('/') {
				Some(0) => ("/", &path[1..]),
				Some(pos) => (&path[..pos], &path[pos + 1..]),
				None => ("/", path),
			};

			match vfs::lookup_path(parent_path) {
				Some(dir) => match dir.mkdir(name) {
					Ok(()) => 0,
					Err(_) => ERRNO_EINVAL,
				},
				None => ERRNO_ENOENT,
			}
		}

		SYS_UNLINK => {
			let ptr = arg1 as *const u8;
			let len = arg2 as usize;

			if !is_user_accessible(ptr, len) {
				return ERRNO_EFAULT;
			}
			let slice = unsafe { core::slice::from_raw_parts(ptr, len) };
			let path = match core::str::from_utf8(slice) {
				Ok(s) => s,
				Err(_) => return ERRNO_EINVAL,
			};

			let (parent_path, name) = match path.rfind('/') {
				Some(0) => ("/", &path[1..]),
				Some(pos) => (&path[..pos], &path[pos + 1..]),
				None => ("/", path),
			};

			match vfs::lookup_path(parent_path) {
				Some(dir) => match dir.unlink(name) {
					Ok(()) => 0,
					Err(_) => ERRNO_ENOENT,
				},
				None => ERRNO_ENOENT,
			}
		}

		_ => {
			/* Unknown system call */
			hal::serial_println!("[SYSCALL] Unknown syscall: {}", nr);
			ERRNO_EINVAL
		}
	}
}
