/*
 * init/scheduler.rs - Scheduler & Task Initialization
 *
 * Provides init_scheduler() which handles:
 * - Global scheduler initialization
 * - Per-CPU run queue setup
 * - Kernel stack region allocation
 * - Boot task creation with IPC ports
 * - Spawning ext4d daemon and kshell
 */

use alloc::sync::Arc;
use task::Scheduler;
use hal::serial_println;

/*
 * init_scheduler - Initialize the task scheduler and spawn initial processes
 * @core_type: CPU topology type (from hal::topology)
 *
 * Initializes the global scheduler, creates the boot task placeholder,
 * and spawns the ext4d daemon and kernel shell.
 */
pub fn init_scheduler() {

	/* Initialize system calls */
	crate::syscall::init_syscalls();

	/* Generate capability handle */
	let cap = capability::CapabilityHandle::generate();
	serial_println!("Generated Secure Capability Handle: {:?}", cap);

	/* Initialize global task scheduler */
	Scheduler::init_global();
	unsafe {
		task::scheduler::init(core::ptr::addr_of!(crate::gdt::PER_CPU_DATA) as usize, 0);
	}
	serial_println!("Kernel task registered");
	graphics::fb_println!("Scheduler: initialized");

	/* Create boot task, IPC ports, and seed run queue */
	let _boot_task = super::process::init_boot_task();
	drop(_boot_task); /* boot_task already seeded the run queue */

	/* Spawn the ext4 filesystem daemon */
	match crate::process::spawn_user_process("/ext4d", 0) {
		Ok(pid) => {
			serial_println!("ext4d: daemon spawned PID={}", pid);
			graphics::fb_println!("ext4d: daemon spawned PID={}", pid);
		}
		Err(e) => {
			serial_println!("ext4d: spawn failed: {}", e);
			graphics::fb_println!("ext4d: spawn failed");
		}
	}

	/* Auto-mount: probe /dev/sda for a known filesystem.
	 * Single-partition assumption: the entire disk.img is one filesystem.
	 * ext4d must be running first since mount() returns Ext4DirStub
	 * which requires IPC to handle lookups.
	 * We mount at /mnt so that embedded ELF files on RamDir (/) remain
	 * accessible for spawning kshell and policyd.
	 * If detection fails, fall back to RamDir at / (no breakage). */
	if let Some(root_ino) = fs::probe_and_mount(Arc::new(fs::VirtioBlockDev::new(0))) {
		serial_println!("VFS: auto-mounted filesystem at /mnt (single-partition)");
		graphics::fb_println!("VFS: /mnt mounted (auto-detected)");
		vfs::mount("/mnt", root_ino);
	} else {
		serial_println!("VFS: no filesystem detected on /dev/sda, using RamDir fallback");
		graphics::fb_println!("VFS: RamDir fallback (run 'mount /dev/sda /mnt' manually)");
	}

	/* Spawn the built-in kernel shell */
	match crate::kshell::spawn_kshell() {
		Ok(pid) => {
			serial_println!("Kernel shell spawned: PID={}", pid);
			graphics::fb_println!("kshell: spawned PID={}", pid);
		}
		Err(e) => {
			serial_println!("Failed to spawn kshell: {}", e);
			graphics::fb_println!("kshell: spawn failed");
		}
	}

	/* Spawn the policy daemon */
	match crate::process::spawn_user_process("/policyd", 0) {
		Ok(pid) => {
			serial_println!("policyd: daemon spawned PID={}", pid);
			graphics::fb_println!("policyd: daemon spawned PID={}", pid);
		}
		Err(e) => {
			serial_println!("policyd: spawn failed: {}", e);
			graphics::fb_println!("policyd: spawn failed");
		}
	}
}
