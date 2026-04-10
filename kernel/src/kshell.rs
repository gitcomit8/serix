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
use alloc::vec::Vec;
use core::fmt::Write as FmtWrite;

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

	graphics::kprint!("ksh> ");

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
				graphics::kprint!("ksh> ");
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
 * Polls PS/2 (bare metal) and COM1 serial (QEMU -serial stdio) so the
 * shell is usable in both environments.  PS/2 is checked first; serial
 * is a fallback and can be removed once PS/2 is validated on bare metal.
 */
fn read_byte() -> u8 {
	loop {
		x86_64::instructions::interrupts::enable();

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
enum RedirMode { Overwrite, Append }

/*
 * parse_line - Split "cmd >> file" / "cmd > file" / "cmd" into parts.
 *
 * Checks `>>` before `>` to avoid misidentifying the first `>`.
 * Returns (command_portion, Some((mode, target_path))).
 */
fn parse_line(line: &str) -> (&str, Option<(RedirMode, &str)>) {
	if let Some(pos) = line.find(">>") {
		let cmd  = line[..pos].trim();
		let file = line[pos+2..].trim();
		return (cmd, Some((RedirMode::Append, file)));
	}
	if let Some(pos) = line.find('>') {
		let cmd  = line[..pos].trim();
		let file = line[pos+1..].trim();
		return (cmd, Some((RedirMode::Overwrite, file)));
	}
	(line, None)
}

/* ------------------------------------------------------------------ */
/*  Command dispatch                                                   */
/* ------------------------------------------------------------------ */

fn dispatch(line: &str) {
	let (cmd_line, redir) = parse_line(line);
	if cmd_line.is_empty() { return; }

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
	let cmd  = parts.next().unwrap_or("");
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
			let _ = writeln!(out, "  halt                 - stop the CPU");
			let _ = writeln!(out, "  reboot               - triple-fault reboot");
		}

		"echo" => {
			let _ = writeln!(out, "{}", args);
		}

		"ls" => {
			let path = if args.is_empty() { "/" } else { args };
			match vfs::lookup_path(path) {
				None => {
					let _ = writeln!(out, "ls: {}: not found", path);
				}
				Some(node) => {
					match node.readdir() {
						Some(entries) => {
							if entries.is_empty() {
								let _ = writeln!(out, "(empty)");
							} else {
								for (name, ft) in entries {
									let tag = match ft {
										vfs::FileType::Directory => "[DIR] ",
										vfs::FileType::Device    => "[DEV] ",
										vfs::FileType::File      => "[FILE]",
									};
									let _ = writeln!(out, "  {} {}", tag, name);
								}
							}
						}
						None => {
							let _ = writeln!(out, "ls: {}: not a directory", path);
						}
					}
				}
			}
		}

		"cat" => {
			if args.is_empty() {
				let _ = writeln!(out, "usage: cat <file>");
				return;
			}
			match vfs::lookup_path(args) {
				None => {
					let _ = writeln!(out, "cat: {}: not found", args);
				}
				Some(node) => {
					let mut offset = 0usize;
					let mut chunk  = [0u8; 512];
					loop {
						let n = node.read(offset, &mut chunk);
						if n == 0 { break; }
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
					if !out.ends_with('\n') { out.push('\n'); }
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
	let cmd  = parts.next().unwrap_or("");
	let arg1 = parts.next().unwrap_or("").trim();
	let arg2 = parts.next().unwrap_or("").trim();

	match cmd {
		"write" => {
			/* write <file> <data...> */
			if arg1.is_empty() {
				graphics::kprintln!("usage: write <file> <data>");
				return;
			}
			let data = arg2.as_bytes();
			match vfs::lookup_path(arg1) {
				Some(node) => {
					node.write(0, data);
				}
				None => {
					/* File doesn't exist — create it in the parent directory */
					if let Some(node) = create_file_at(arg1) {
						node.write(0, data);
					} else {
						graphics::kprintln!("write: {}: cannot create", arg1);
					}
				}
			}
		}

		"mkdir" => {
			if arg1.is_empty() {
				graphics::kprintln!("usage: mkdir <path>");
				return;
			}
			let (parent_path, name) = split_path(arg1);
			match vfs::lookup_path(parent_path) {
				Some(parent) => {
					match parent.mkdir(name) {
						Ok(()) => {}
						Err(e) => graphics::kprintln!("mkdir: {}", e),
					}
				}
				None => graphics::kprintln!("mkdir: {}: parent not found", parent_path),
			}
		}

		"rm" => {
			if arg1.is_empty() {
				graphics::kprintln!("usage: rm <path>");
				return;
			}
			let (parent_path, name) = split_path(arg1);
			match vfs::lookup_path(parent_path) {
				Some(parent) => {
					match parent.unlink(name) {
						Ok(()) => {}
						Err(e) => graphics::kprintln!("rm: {}", e),
					}
				}
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
 * split_path - Split "/a/b/c" into ("/a/b", "c").
 *
 * Returns ("/", name) when there is no parent component.
 */
fn split_path(path: &str) -> (&str, &str) {
	let trimmed = path.trim_end_matches('/');
	match trimmed.rfind('/') {
		None          => ("/", trimmed),
		Some(0)       => ("/", &trimmed[1..]),
		Some(pos)     => (&trimmed[..pos], &trimmed[pos+1..]),
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
		None    => match create_file_at(path) {
			Some(n) => n,
			None    => {
				graphics::kprintln!("redirect: {}: cannot create", path);
				return;
			}
		}
	};
	let offset = match mode {
		RedirMode::Overwrite => 0,
		RedirMode::Append    => node.size(),
	};
	node.write(offset, data);
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
	let kstack = memory::kstack::alloc_kernel_stack(64 * 1024)
		.ok_or("kshell: OOM allocating stack")?;

	let id     = task::TaskId::new();
	let id_val = id.0;

	let mut ctx = task::CPUContext::default();
	ctx.rsp    = kstack.as_u64();
	ctx.rip    = kshell_task as u64;
	ctx.cr3    = 0;      /* 0 = keep kernel CR3 */
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
