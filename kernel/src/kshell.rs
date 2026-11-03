/*
 * kshell.rs - Kernel-space Interactive Shell
 *
 * A built-in TTY shell running as a Ring-0 kernel task.
 *
 * Input:  PS/2 keyboard only (interrupt-driven via keyboard::pop_key).
 * Output: Framebuffer console only (graphics::kprint!/kprintln!).
 *
 * Spawned by spawn_kshell() which allocates a kernel stack and enqueues
 * the task before the timer starts.
 *
 * Commands: help, echo, ls, cat, write, mkdir, rm, mount, umount, halt, reboot
 * I/O:      cmd > file   (overwrite)
 *           cmd >> file  (append)
 */

extern crate alloc;

use alloc::string::String;
use core::fmt::Write as FmtWrite;

/*
 * syscall4 - Direct syscall wrapper for kernel-side syscalls.
 *
 * kshell runs as a Ring-0 task, so it can invoke syscalls directly
 * without going through the userspace ulib wrappers.
 */
unsafe fn syscall4(nr: usize, arg1: usize, arg2: usize, arg3: usize, arg4: usize) -> usize {
	let ret: usize;
	core::arch::asm!(
		"syscall",
		in("rax") nr,
		in("rdi") arg1,
		in("rsi") arg2,
		in("rdx") arg3,
		in("r10") arg4,
		lateout("rax") ret,
		out("rcx") _,
		out("r11") _,
	);
	ret
}

const LINE_MAX: usize = 256;

/* ------------------------------------------------------------------ */
/*  Shell entry point                                                  */
/* ------------------------------------------------------------------ */

/*
 * kshell_task - Entry point for the kernel shell task.
 *
 * Must be `unsafe extern "C" fn() -> !` to satisfy TaskCB::new.
 */
pub unsafe extern "C" fn kshell_task() -> ! {
	graphics::kprintln!();
	graphics::kprintln!("=== Serix kshell ===");
	graphics::kprintln!("Type 'help' for commands.");
	graphics::kprintln!();

	let mut buf = [0u8; LINE_MAX];
	let mut len: usize = 0;

	graphics::set_kernel_fg_color(0x0000FF00); // Green
	graphics::kprint!("ksh> ");
	graphics::set_kernel_fg_color(0x00AAAAAA); // Restore gray

	loop {
		let byte = read_byte();

		match byte {
			b'\r' | b'\n' => {
				graphics::kprintln!();
				let line = core::str::from_utf8(&buf[..len]).unwrap_or("").trim();
				if !line.is_empty() {
					dispatch(line);
				}
				len = 0;
				graphics::set_kernel_fg_color(0x0000FF00); // Green
				graphics::kprint!("ksh> ");
				graphics::set_kernel_fg_color(0x00AAAAAA); // Restore gray
			}
			/* Backspace / DEL */
			0x08 | 0x7F => {
				if len > 0 {
					len -= 1;
					/* Erase last character on screen: BS + space + BS */
					graphics::kprint!("\x08 \x08");
				}
			}
			/* Printable ASCII */
			b if b >= 0x20 && b < 0x7F => {
				if len < LINE_MAX - 1 {
					buf[len] = b;
					len += 1;
					let s = core::str::from_utf8(core::slice::from_ref(&b)).unwrap_or("?");
					graphics::kprint!("{}", s);
				}
			}
			_ => {}
		}
	}
}

/* ------------------------------------------------------------------ */
/*  Input                                                              */
/* ------------------------------------------------------------------ */

/*
 * read_byte - Block until a key arrives.
 *
 * PS/2 keyboard is checked first so QEMU window input works directly.
 * Serial (COM1) is checked second as a fallback for -serial stdio mode.
 */
fn read_byte() -> u8 {
	let mut observed_isr_count = keyboard::get_isr_fire_count();
	loop {
		x86_64::instructions::interrupts::enable();

		let isr_count = keyboard::get_isr_fire_count();
		if isr_count != observed_isr_count {
			observed_isr_count = isr_count;
			hal::serial_println!(
				"[DIAG] Keyboard IRQ count={}, last scancode={:#04x}",
				isr_count,
				keyboard::get_last_scancode()
			);
		}

		if let Some(b) = keyboard::pop_key() {
			x86_64::instructions::interrupts::disable();
			return b;
		}

		if let Some(b) = hal::serial::serial_read_byte() {
			x86_64::instructions::interrupts::disable();
			return b;
		}

		core::hint::spin_loop();
	}
}

/* ------------------------------------------------------------------ */
/*  I/O redirection                                                    */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq)]
enum RedirMode {
	Overwrite,
	Append,
}

/*
 * parse_line - Split "cmd >> file" / "cmd > file" / "cmd" into parts.
 *
 * Checks `>>` before `>` to avoid misidentifying the first `>`.
 * Returns (command_portion, Some((mode, target_path))).
 */
fn parse_line(line: &str) -> (&str, Option<(RedirMode, &str)>) {
	if let Some(pos) = line.find(">>") {
		let cmd = line[..pos].trim();
		let file = line[pos + 2..].trim();
		return (cmd, Some((RedirMode::Append, file)));
	}
	if let Some(pos) = line.find('>') {
		let cmd = line[..pos].trim();
		let file = line[pos + 1..].trim();
		return (cmd, Some((RedirMode::Overwrite, file)));
	}
	(line, None)
}

/* ------------------------------------------------------------------ */
/*  Command dispatch                                                   */
/* ------------------------------------------------------------------ */

fn dispatch(line: &str) {
	let (cmd_line, redir) = parse_line(line);
	if cmd_line.is_empty() {
		return;
	}

	/* Commands that produce text output — buffered so we can redirect */
	let is_output_cmd = {
		let first = cmd_line.split_whitespace().next().unwrap_or("");
		matches!(first, "ls" | "cat" | "echo" | "help")
	};

	if is_output_cmd {
		let mut out = String::new();
		run_output_command(cmd_line, &mut out);

		match redir {
			None => {
				/* Print to screen */
				graphics::kprint!("{}", out);
			}
			Some((mode, path)) => {
				write_to_file(path, out.as_bytes(), mode);
			}
		}
	} else {
		/* Side-effect commands — run directly, redirection is silently ignored */
		run_side_effect_command(cmd_line);
	}
}

/* ------------------------------------------------------------------ */
/*  Output commands (write into a String buffer)                      */
/* ------------------------------------------------------------------ */

fn run_output_command(line: &str, out: &mut String) {
	let mut parts = line.splitn(2, ' ');
	let cmd = parts.next().unwrap_or("");
	let args = parts.next().unwrap_or("").trim();

	match cmd {
		"help" => {
			let _ = writeln!(out, "Available commands:");
			let _ = writeln!(out, "  help                 - show this message");
			let _ = writeln!(out, "  echo <text>          - print text");
			let _ = writeln!(out, "  ls [path]            - list directory");
			let _ = writeln!(out, "  cat <file>           - print file contents");
			let _ = writeln!(out, "  write <file> <data>  - write data to file");
			let _ = writeln!(out, "  mkdir <path>         - create directory");
			let _ = writeln!(out, "  rm <path>            - remove file");
			let _ = writeln!(out, "  mount <dev> <path>   - mount filesystem");
			let _ = writeln!(out, "  umount <path>        - unmount filesystem");
			let _ = writeln!(out, "  policy list          - show active policy rules");
			let _ = writeln!(out, "  policy reload [path] - reload policy from config");
			let _ = writeln!(out, "  halt                 - stop the CPU");
			let _ = writeln!(out, "  reboot               - triple-fault reboot");
		}

		"echo" => {
			let _ = writeln!(out, "{}", args);
		}

		"ls" => {
			let p = if args.is_empty() {
				String::from("/")
			} else {
				abs_path(args)
			};
			match vfs::lookup_path(&p) {
				None => {
					let _ = writeln!(out, "ls: {}: not found", p);
				}
				Some(node) => match node.readdir() {
					Some(entries) => {
						if entries.is_empty() {
							let _ = writeln!(out, "(empty)");
						} else {
							for (name, ft) in entries {
								let tag = match ft {
									vfs::FileType::Directory => "[DIR] ",
									vfs::FileType::Device => "[DEV] ",
									vfs::FileType::File => "[FILE]",
								};
								let _ = writeln!(out, "  {} {}", tag, name);
							}
						}
					}
					None => {
						let _ = writeln!(out, "ls: {}: not a directory", p);
					}
				},
			}
		}

		"cat" => {
			if args.is_empty() {
				let _ = writeln!(out, "usage: cat <file>");
				return;
			}
			let p = abs_path(args);
			match vfs::lookup_path(&p) {
				None => {
					let _ = writeln!(out, "cat: {}: not found", p);
				}
				Some(node) => {
					let mut offset = 0usize;
					let mut chunk = [0u8; 512];
					loop {
						let n = node.read(offset, &mut chunk);
						if n == 0 {
							break;
						}
						if let Ok(s) = core::str::from_utf8(&chunk[..n]) {
							out.push_str(s);
						} else {
							/* Non-UTF8: show as hex pairs */
							for b in &chunk[..n] {
								let _ = write!(out, "{:02x} ", b);
							}
						}
						offset += n;
					}
					/* Ensure trailing newline */
					if !out.ends_with('\n') {
						out.push('\n');
					}
				}
			}
		}

		_ => {
			let _ = writeln!(out, "ksh: {}: command not found", cmd);
		}
	}
}

/* ------------------------------------------------------------------ */
/*  Side-effect commands (no buffering needed)                        */
/* ------------------------------------------------------------------ */

fn run_side_effect_command(line: &str) {
	let mut parts = line.splitn(3, ' ');
	let cmd = parts.next().unwrap_or("");
	let arg1 = parts.next().unwrap_or("").trim();
	let arg2 = parts.next().unwrap_or("").trim();

	match cmd {
		"policy" => handle_policy_command(arg1, arg2),
		"write" => {
			/* write <file> <data...> */
			if arg1.is_empty() {
				graphics::kprintln!("usage: write <file> <data>");
				return;
			}
			let p = abs_path(arg1);
			let data = arg2.as_bytes();
			match vfs::lookup_path(&p) {
				Some(node) => {
					node.write(0, data);
				}
				None => {
					if let Some(node) = create_file_at(&p) {
						node.write(0, data);
					} else {
						graphics::kprintln!("write: {}: cannot create", p);
					}
				}
			}
		}

		"mkdir" => {
			if arg1.is_empty() {
				graphics::kprintln!("usage: mkdir <path>");
				return;
			}
			let p = abs_path(arg1);
			let (parent_path, name) = split_path(&p);
			match vfs::lookup_path(parent_path) {
				Some(parent) => match parent.mkdir(name) {
					Ok(()) => {}
					Err(e) => graphics::kprintln!("mkdir: {}", e),
				},
				None => graphics::kprintln!("mkdir: {}: parent not found", parent_path),
			}
		}

		"rm" => {
			if arg1.is_empty() {
				graphics::kprintln!("usage: rm <path>");
				return;
			}
			let p = abs_path(arg1);
			let (parent_path, name) = split_path(&p);
			match vfs::lookup_path(parent_path) {
				Some(parent) => match parent.unlink(name) {
					Ok(()) => {}
					Err(e) => graphics::kprintln!("rm: {}", e),
				},
				None => graphics::kprintln!("rm: {}: parent not found", parent_path),
			}
		}

		"mount" => {
			/* mount <dev_path> <mount_path> */
			if arg1.is_empty() || arg2.is_empty() {
				graphics::kprintln!("usage: mount <dev> <path>");
				return;
			}
			/* Look up the block device node */
			let dev_node = match vfs::lookup_path(arg1) {
				Some(n) => n,
				None => {
					graphics::kprintln!("mount: {}: not found", arg1);
					return;
				}
			};
			/* Cast to BlockDevINode via fs::BlockDevINode downcast not possible in no_std
			 * Instead, wrap the device node's read/write in an adapter. */
			let block_dev = alloc::sync::Arc::new(VfsBlockDevAdapter(dev_node));
			match fs::probe_and_mount(block_dev) {
				Some(root) => {
					vfs::mount(arg2, root);
					graphics::kprintln!("mount: {} mounted at {}", arg1, arg2);
				}
				None => {
					graphics::kprintln!("mount: {}: no filesystem recognised", arg1);
				}
			}
		}

		"umount" => {
			if arg1.is_empty() {
				graphics::kprintln!("usage: umount <path>");
				return;
			}
			match vfs::umount(arg1) {
				Ok(()) => graphics::kprintln!("umount: {} unmounted", arg1),
				Err(e) => graphics::kprintln!("umount: {}", e),
			}
		}

		"halt" => {
			graphics::kprintln!("Halting system.");
			x86_64::instructions::interrupts::disable();
			loop {
				x86_64::instructions::hlt();
			}
		}

		"reboot" => {
			graphics::kprintln!("Rebooting...");
			unsafe {
				core::arch::asm!(
					"sub rsp, 10",
					"mov word ptr [rsp], 0",
					"mov qword ptr [rsp+2], 0",
					"lidt [rsp]",
					"int 0",
					options(nostack, noreturn)
				);
			}
		}

		_ => {
			graphics::kprintln!("ksh: {}: command not found", cmd);
		}
	}
}

/* ------------------------------------------------------------------ */
/*  VFS block device adapter for mount                                */
/* ------------------------------------------------------------------ */

/*
 * VfsBlockDevAdapter - Wraps a VFS INode as a BlockDev.
 *
 * Used by the `mount` command to pass /dev/sda (a BlockDevINode in the VFS)
 * into fs::probe_and_mount() which expects an Arc<dyn BlockDev>.
 */
struct VfsBlockDevAdapter(alloc::sync::Arc<dyn vfs::INode>);

impl fs::BlockDev for VfsBlockDevAdapter {
	fn read_block(&self, sector: u64, buf: &mut [u8; 512]) -> bool {
		let n = self.0.read((sector as usize) * 512, buf);
		n == 512
	}
	fn write_block(&self, sector: u64, buf: &[u8; 512]) -> bool {
		let n = self.0.write((sector as usize) * 512, buf);
		n == 512
	}
	fn sector_count(&self) -> u64 {
		(self.0.size() / 512) as u64
	}
}

/* ------------------------------------------------------------------ */
/*  Helpers                                                            */
/* ------------------------------------------------------------------ */

/*
 * abs_path - Return path with a leading '/' if not already present.
 *
 * kshell treats all paths as absolute from '/'.  This lets users type
 * 'ls etc' instead of 'ls /etc' without confusion.
 */
fn abs_path(path: &str) -> String {
	if path.starts_with('/') {
		String::from(path)
	} else {
		let mut s = String::from("/");
		s.push_str(path);
		s
	}
}

/*
 * split_path - Split "/a/b/c" into ("/a/b", "c").
 *
 * Returns ("/", name) when there is no parent component.
 */
fn split_path(path: &str) -> (&str, &str) {
	let trimmed = path.trim_end_matches('/');
	match trimmed.rfind('/') {
		None => ("/", trimmed),
		Some(0) => ("/", &trimmed[1..]),
		Some(pos) => (&trimmed[..pos], &trimmed[pos + 1..]),
	}
}

/*
 * create_file_at - Look up parent directory and call create_file().
 *
 * Returns the new INode or None on failure.
 */
fn create_file_at(path: &str) -> Option<alloc::sync::Arc<dyn vfs::INode>> {
	let (parent_path, name) = split_path(path);
	let parent = vfs::lookup_path(parent_path)?;
	parent.create_file(name).ok()
}

/*
 * write_to_file - Write bytes to a VFS path (create if necessary).
 */
fn write_to_file(path: &str, data: &[u8], mode: RedirMode) {
	let node = match vfs::lookup_path(path) {
		Some(n) => n,
		None => match create_file_at(path) {
			Some(n) => n,
			None => {
				graphics::kprintln!("redirect: {}: cannot create", path);
				return;
			}
		},
	};
	let offset = match mode {
		RedirMode::Overwrite => 0,
		RedirMode::Append => node.size(),
	};
	node.write(offset, data);
}

/* ------------------------------------------------------------------ */
/*  Policy management                                                 */
/* ------------------------------------------------------------------ */

/*
 * handle_policy_command - Handle `policy` subcommands from kshell.
 *
 * Subcommands:
 *   list       - Read and display the policy config file
 *   reload     - Reload policy from config (with optional custom path)
 */
fn handle_policy_command(subcmd: &str, arg: &str) {
	match subcmd {
		"list" => policy_list(),
		"reload" => policy_reload(arg),
		_ => {
			graphics::kprintln!("usage: policy list|reload [path]");
		}
	}
}

/*
 * policy_list - Read and display the policy config file.
 *
 * Reads /etc/serix/policy.toml from the VFS and prints its contents
 * to the framebuffer console.
 */
fn policy_list() {
	let path = "/etc/serix/policy.toml";
	let node = match vfs::lookup_path(path) {
		Some(n) => n,
		None => {
			graphics::kprintln!("policy: {} not found", path);
			return;
		}
	};

	let size = node.size();
	if size == 0 {
		graphics::kprintln!("policy: {} is empty", path);
		return;
	}

	let mut offset = 0usize;
	let mut chunk = [0u8; 512];
	graphics::kprintln!("=== Active Policy Rules (from {}) ===", path);

	while offset < size {
		let remaining = size - offset;
		let chunk_size = core::cmp::min(remaining, chunk.len());
		let n = node.read(offset, &mut chunk[..chunk_size]);
		if n == 0 {
			break;
		}

		if let Ok(s) = core::str::from_utf8(&chunk[..n]) {
			graphics::kprint!("{}", s);
		} else {
			/* Non-UTF8: show as hex pairs */
			for b in &chunk[..n] {
				graphics::kprint!("{:02x} ", b);
			}
		}
		offset += n;
	}

	graphics::kprintln!();
}

/*
 * policy_reload - Reload policy from config file.
 *
 * Sends a SYS_POLICY_RELOAD syscall to reload the LES policy snapshot
 * and notify policyd to reload its config.
 *
 * If a path is provided, uses that path instead of the default.
 */
fn policy_reload(path: &str) {
	let config_path = if path.is_empty() {
		"/etc/serix/policy.toml"
	} else {
		path
	};

	/* Use a syscall to reload the policy */
	let ret = unsafe {
		syscall4(93u64 as usize,  /* SYS_POLICY_RELOAD */
			3,                              /* root cap slot */
			1,                              /* root cap gen */
			config_path.as_ptr() as usize,
			config_path.len())
	};

	if ret == 0 {
		graphics::kprintln!("policy: reloaded from {}", config_path);
	} else {
		graphics::kprintln!("policy: reload failed (errno={})", ret);
	}
}

/* ------------------------------------------------------------------ */
/*  Task spawn                                                         */
/* ------------------------------------------------------------------ */

/*
 * spawn_kshell - Allocate a kernel stack and enqueue the kshell task.
 *
 * Called from _start after the kstack region and scheduler are ready.
 */
pub fn spawn_kshell() -> Result<u64, &'static str> {
	let kstack =
		memory::kstack::alloc_kernel_stack(64 * 1024).ok_or("kshell: OOM allocating stack")?;

	let id = task::TaskId::new();
	let id_val = id.0;

	let mut ctx = task::CPUContext::default();
	ctx.rsp = kstack.as_u64();
	ctx.rip = kshell_task as u64;
	ctx.cr3 = 0; /* 0 = keep kernel CR3 */
	ctx.cs = 0x08;
	ctx.ss = 0x10;
	ctx.rflags = 0x202;

	let tcb = task::TaskCB {
		id,
		state: task::TaskState::Ready,
		sched_class: task::SchedClass::Fair(120),
		context: ctx,
		kstack,
		ustack: None,
		name: "kshell",
		parent_id: 0,
		exit_status: None,
		pml4_frame: None,
		children: alloc::vec::Vec::new(),
		waiting_for_child: false,
		cspace: alloc::sync::Arc::new(spin::Mutex::new(capability::cspace::CapabilitySpace::new())),
		virtual_runtime: 0,
		inherited_priority: None,
		blocked_on: None,
		direct_msg_ptr: core::sync::atomic::AtomicUsize::new(0),
		direct_msg_valid: core::sync::atomic::AtomicBool::new(false),
		vma_manager: task::VmaManager::new(),
	};

	task::scheduler::enqueue_task(alloc::sync::Arc::new(spin::Mutex::new(tcb)));
	Ok(id_val)
}
