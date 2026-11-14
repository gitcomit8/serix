/*
 * process.rs - User Process Creation
 *
 * Provides spawn_user_process() which loads an ELF from the VFS,
 * creates a new address space, and enqueues the task for scheduling.
 *
 * Ring 3 entry is performed by user_entry_trampoline, a naked function
 * that context_switch ret's into on the task's first time slice.
 */

extern crate alloc;

use alloc::sync::Arc;
use alloc::vec;
use alloc::vec::Vec;
use core::arch::naked_asm;
use loader::LoadableSegment;
use spin::Mutex;
use x86_64::VirtAddr;
use x86_64::structures::paging::{
	FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB, Translate,
};

/*
 * user_entry_trampoline - Ring 0 → Ring 3 bridge for newly spawned tasks
 *
 * context_switch() ret's here on the task's first scheduled time slice.
 * At entry: kernel stack is valid, r12 = user entry point, r13 = user RSP.
 * Builds the iretq frame and switches to Ring 3.
 *
 * GS invariant (this kernel): GS_BASE is ALWAYS 0 in both Ring 0 and Ring 3
 * normal execution. KernelGsBase holds PER_CPU_DATA. swapgs only happens
 * inside syscall_entry/syscall_exit to temporarily make PER_CPU_DATA live.
 *
 * Therefore this trampoline must always be reached with GS_BASE=0. Callers
 * in syscall context (SYS_EXIT, SYS_WAIT) must issue swapgs before calling
 * context_switch so that GS is restored before we arrive here.
 */
#[unsafe(naked)]
pub unsafe extern "C" fn user_entry_trampoline() -> ! {
	naked_asm!(
		"cli",
		"push 0x23",  /* SS  — user data RPL 3 */
		"push r13",   /* RSP — user stack pointer */
		"push 0x202", /* RFLAGS — IF=1, reserved=1 */
		"push 0x2B",  /* CS  — user code RPL 3 */
		"push r12",   /* RIP — user entry point */
		"iretq",
	)
}

/*
 * map_segment - Map one ELF PT_LOAD segment into a user address space
 * @mapper:     Mapper for the target PML4 (not necessarily the active one)
 * @allocator:  Physical frame allocator
 * @segment:    Segment descriptor from the loader
 * @phys_offset: HHDM offset used to access frame contents
 */
pub unsafe fn map_segment(
	mapper: &mut (impl Mapper<Size4KiB> + Translate),
	allocator: &mut impl FrameAllocator<Size4KiB>,
	segment: &LoadableSegment,
	phys_offset: VirtAddr,
) {
	use x86_64::structures::paging::mapper::MapToError;

	let start = segment.virtual_address;
	let end = start + segment.size;
	let start_page = Page::<Size4KiB>::containing_address(start);
	let end_page = Page::<Size4KiB>::containing_address(end - 1u64);

	let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
	if segment.flags.writable {
		flags |= PageTableFlags::WRITABLE;
	}
	if !segment.flags.executable {
		flags |= PageTableFlags::NO_EXECUTE;
	}

	for page in Page::range_inclusive(start_page, end_page) {
		let (frame, freshly_mapped) =
			if let Some(existing_phys) = mapper.translate_addr(page.start_address()) {
				let frame = PhysFrame::containing_address(existing_phys);
				/* Overlapping ELF segments may require adding permissions on an existing page. */
				mapper
					.update_flags(page, flags)
					.expect("map_segment: update_flags failed")
					.flush();
				(frame, false)
			} else {
				let new_frame = allocator.allocate_frame().expect("OOM during segment load");
				match mapper.map_to(page, new_frame, flags, allocator) {
					Ok(f) => {
						f.flush();
						(new_frame, true)
					}
					Err(MapToError::PageAlreadyMapped(_)) => {
						let phys = mapper
							.translate_addr(page.start_address())
							.expect("map_segment: mapped page missing translation");
						(PhysFrame::containing_address(phys), false)
					}
					Err(e) => panic!("map_segment: {:?}", e),
				}
			};

		let frame_virt = phys_offset + frame.start_address().as_u64();
		let ptr = frame_virt.as_mut_ptr::<u8>();
		if freshly_mapped {
			ptr.write_bytes(0, 4096);
		}

		let page_addr = page.start_address().as_u64();
		let seg_addr = start.as_u64();

		let data_start = if page_addr < seg_addr {
			0
		} else {
			page_addr - seg_addr
		};
		let data_end = core::cmp::min(
			segment.data.len() as u64,
			(page_addr + 4096).saturating_sub(seg_addr),
		);

		if data_start < data_end {
			let dest_offset = if page_addr < seg_addr {
				seg_addr - page_addr
			} else {
				0
			};
			core::ptr::copy_nonoverlapping(
				segment.data.as_ptr().add(data_start as usize),
				ptr.add(dest_offset as usize),
				(data_end - data_start) as usize,
			);
		}
	}
}

/*
 * allocate_user_stack - Map a 16 KiB user stack near the top of the lower half
 * @mapper:     Mapper for the target address space
 * @allocator:  Physical frame allocator
 * @phys_offset: HHDM offset
 *
 * Return: Virtual address of the stack top (initial user RSP)
 */
pub unsafe fn allocate_user_stack(
	mapper: &mut impl Mapper<Size4KiB>,
	allocator: &mut impl FrameAllocator<Size4KiB>,
	phys_offset: VirtAddr,
) -> VirtAddr {
	let stack_top = VirtAddr::new(0x0000_7FFF_FFFF_F000);
	let stack_bottom = stack_top - 16384u64;

	let start_page = Page::<Size4KiB>::containing_address(stack_bottom);
	let end_page = Page::<Size4KiB>::containing_address(stack_top - 1u64);
	let flags =
		PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

	for page in Page::range_inclusive(start_page, end_page) {
		let frame = allocator.allocate_frame().expect("OOM: user stack");
		if let Ok(r) = mapper.map_to(page, frame, flags, allocator) {
			r.flush();
		}
		let ptr = (phys_offset + frame.start_address().as_u64()).as_mut_ptr::<u8>();
		ptr.write_bytes(0, 4096);
	}
	stack_top
}

/*
 * write_stack_bytes - Copy bytes into a freshly mapped user stack
 * @mapper:      Address space mapper
 * @phys_offset: HHDM offset
 * @start:       Virtual address the first byte lands at
 * @bytes:       Data to copy
 *
 * The stack pages were mapped moments ago, so each one translates to a
 * frame that is written through the HHDM.
 */
unsafe fn write_stack_bytes(
	mapper: &impl Translate,
	phys_offset: VirtAddr,
	start: VirtAddr,
	bytes: &[u8],
) {
	let mut done = 0usize;
	while done < bytes.len() {
		let va = start + done as u64;
		let page = Page::<Size4KiB>::containing_address(va);
		let in_page = (va.as_u64() - page.start_address().as_u64()) as usize;
		let chunk = core::cmp::min(4096 - in_page, bytes.len() - done);
		if let Some(phys) = mapper.translate_addr(va) {
			let dst = (phys_offset + phys.as_u64()).as_mut_ptr::<u8>();
			core::ptr::copy_nonoverlapping(bytes[done..].as_ptr(), dst, chunk);
		}
		done += chunk;
	}
}

/*
 * build_arg_block - Lay out argc, argv, envp, and their strings
 * @stack_top: Top of the user stack
 * @argv:      Arguments; argv[0] is the invoked name
 * @envp:      Environment strings
 *
 * Returns the stack pointer to start the program at, plus a buffer whose
 * first byte lives at that address. The layout is the one in
 * docs/USERLAND.md section 6: argc at [rsp], argv pointers above it, a NULL,
 * envp pointers, a NULL, then the strings.
 */
fn build_arg_block(stack_top: VirtAddr, argv: &[&str], envp: &[&str]) -> (VirtAddr, Vec<u8>) {
	let slots = 1 + argv.len() + 1 + envp.len() + 1;
	let header = slots * 8;
	let strings: usize = argv.iter().chain(envp.iter()).map(|s| s.len() + 1).sum();
	let total = header + strings;
	let rsp = VirtAddr::new((stack_top.as_u64() - total as u64) & !0xF);
	let base = rsp.as_u64();
	let mut buf = vec![0u8; total];

	fn put(buf: &mut [u8], at: usize, value: u64) {
		buf[at..at + 8].copy_from_slice(&value.to_le_bytes());
	}

	put(&mut buf, 0, argv.len() as u64);

	let mut string_va = base + header as u64;
	let mut slot = 8usize;
	for arg in argv {
		put(&mut buf, slot, string_va);
		let at = (string_va - base) as usize;
		buf[at..at + arg.len()].copy_from_slice(arg.as_bytes());
		string_va += arg.len() as u64 + 1;
		slot += 8;
	}
	slot += 8; /* argv terminator */
	for env in envp {
		put(&mut buf, slot, string_va);
		let at = (string_va - base) as usize;
		buf[at..at + env.len()].copy_from_slice(env.as_bytes());
		string_va += env.len() as u64 + 1;
		slot += 8;
	}

	(rsp, buf)
}

/*
 * spawn_user_process - Create and enqueue a new user-mode process
 * @path:      VFS path to the ELF binary
 * @parent_id: Task ID of the spawning task (0 = kernel)
 *
 * Return: child task ID on success, Err string on failure
 *
 * Allocates a new PML4, maps ELF segments, sets up a user stack and
 * kernel stack, initialises stdio fds, and enqueues on the RunQueue.
 */
pub fn spawn_user_process(path: &str, parent_id: u64) -> Result<u64, &'static str> {
	spawn_user_process_args(path, parent_id, &[path], &[])
}

/*
 * spawn_user_process_args - Create a user process with arguments
 * @path:      VFS path to the ELF binary
 * @parent_id: Task ID of the spawning task (0 = kernel)
 * @argv:      Arguments laid out for the child; argv[0] is the invoked name
 * @envp:      Environment strings, empty for now
 */
pub fn spawn_user_process_args(
	path: &str,
	parent_id: u64,
	argv: &[&str],
	envp: &[&str],
) -> Result<u64, &'static str> {
	/* 1. Read ELF from VFS */
	let inode = vfs::lookup_path(path).ok_or("spawn: path not found")?;
	let size = inode.size();
	if size == 0 {
		return Err("spawn: ELF is empty");
	}
	let mut data: Vec<u8> = vec![0u8; size];
	let mut off = 0usize;
	while off < size {
		let n = inode.read(off, &mut data[off..]);
		if n == 0 {
			break;
		}
		off += n;
	}

	/* 2. Parse ELF segments */
	let image = loader::load_elf(&data)?;

	/* 3-6. Build address space under PAGE_ALLOC lock */
	let phys_offset = memory::hhdm_offset();

	let (pml4_frame, user_stack_top, kstack): (PhysFrame, VirtAddr, VirtAddr) = {
		let mut alloc_guard = memory::PAGE_ALLOC
			.get()
			.ok_or("spawn: PAGE_ALLOC not ready")?
			.lock();

		let pml4 = unsafe {
			memory::create_user_page_table(&mut alloc_guard.frame_alloc, phys_offset)
				.ok_or("spawn: OOM creating PML4")?
		};

		let mut user_mapper = unsafe { memory::create_mapper(pml4, phys_offset) };

		for seg in &image.segments {
			unsafe {
				map_segment(
					&mut user_mapper,
					&mut alloc_guard.frame_alloc,
					seg,
					phys_offset,
				);
			}
		}

		let ust = unsafe {
			allocate_user_stack(&mut user_mapper, &mut alloc_guard.frame_alloc, phys_offset)
		};

		/* Lay out argc/argv/envp before the address space leaves our hands:
		 * writing them needs the stack frames, which only the mapper knows. */
		let (user_rsp, arg_block) = build_arg_block(ust, argv, envp);
		unsafe {
			write_stack_bytes(&user_mapper, phys_offset, user_rsp, &arg_block);
		}

		drop(alloc_guard); /* release before kernel stack alloc */

		/* Allocate kernel stack from dedicated fixed range (always mapped) */
		let ks =
			memory::kstack::alloc_kernel_stack(1024 * 1024).ok_or("spawn: OOM kernel stack")?;

		(pml4, user_rsp, ks)
	};

	/* 7. Build CPUContext — trampoline runs in Ring 0, then iretq to Ring 3 */
	let mut ctx = task::CPUContext::default();
	ctx.rsp = kstack.as_u64();
	ctx.rip = user_entry_trampoline as u64;
	ctx.r12 = image.entry_point.as_u64(); /* user entry point, read by trampoline */
	ctx.r13 = user_stack_top.as_u64(); /* user RSP, read by trampoline */
	ctx.cr3 = pml4_frame.start_address().as_u64();
	ctx.cs = 0x08; /* kernel CS — trampoline is Ring 0 code */
	ctx.ss = 0x10;
	ctx.rflags = 0x202;

	/* 8. Allocate task ID and build TaskCB */
	let child_id_val = task::TaskId::new();
	let child_id = child_id_val.0;

	let mut tcb = task::TaskCB {
		id: child_id_val,
		state: task::TaskState::Ready,
		sched_class: task::SchedClass::Fair(120),
		context: ctx,
		kstack,
		ustack: Some(user_stack_top),
		name: "user_proc",
		parent_id,
		exit_status: None,
		pml4_frame: Some(pml4_frame),
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

	/* 9. Initialise stdio fds — must come before any capability grants
	 * so that stdin/stdout/stderr occupy the well-known slots 0/1/2
	 * in the task's C-space. This ensures ulib's hard-coded
	 * STDIN_CAP and STDOUT_CAP resolve correctly. */
	crate::fd::init_stdio(child_id);

	/* 10. Bootstrap grants are explicit, least-privilege child capabilities.
	 * They replace the former assumption that a well-known port number
	 * conveys authority. For ext4d, IPC slots come after stdio
	 * (slots 3 = request/RECV, 4 = reply/SEND), and the root directory
	 * inode lands at slot 5. For policyd, IPC slots are slots 3 (RECV)
	 * and 4 (SEND). For all other processes the root inode
	 * is the next capability (slot 3). */
	if path == "/ext4d" {
		grant_bootstrap_ipc(&mut tcb, fs::ext4::ipc::EXT4_REQ_PORT, capability::Rights::RECV)?;
		grant_bootstrap_ipc(&mut tcb, fs::ext4::ipc::EXT4_REPLY_BASE, capability::Rights::SEND)?;
	}

	if path == "/policyd" {
		grant_bootstrap_ipc(&mut tcb, 0x5000, capability::Rights::RECV)?;
		grant_bootstrap_ipc(&mut tcb, 0x5001, capability::Rights::SEND)?;
	}

	/* Grant a LOOKUP capability on the root directory to all user
	 * processes. This allows them to open files relative to /
	 * (e.g., /hello.txt). Always granted after stdio and ext4d IPC caps. */
	if let Some(root_inode) = vfs::lookup_path("/") {
		let ptr = Arc::as_ptr(&root_inode) as *const () as usize as u64;
		let cap_type = capability::CapabilityType::Inode { ptr };
		let rights = capability::Rights::LOOKUP | capability::Rights::READ;
		let cap_handle = capability::CapabilityHandle::generate();
		let record = capability::global_cap_store()
			.lock()
			.mint_root(cap_handle, cap_type, rights, 0);
		tcb.cspace.lock().insert(record.id, false);
	}

	/* 11. Register child in parent's children list */
	if parent_id != 0 {
		if let Some(parent_arc) = task::scheduler::find_task_by_id(parent_id) {
			parent_arc.lock().children.push(child_id);
		}
	}

	/* 12. Enqueue */
	task::scheduler::enqueue_task(Arc::new(Mutex::new(tcb)));

	hal::serial_println!(
		"[SPAWN] pid={} entry={:#x} cr3={:#x}",
		child_id,
		image.entry_point.as_u64(),
		pml4_frame.start_address().as_u64()
	);

	Ok(child_id)
}

/* grant_bootstrap_ipc - Derive a service's initial IPC authority from the
 * boot task's root capability. This is a controlled kernel bootstrap action,
 * not a raw-ID authorization path. */
fn grant_bootstrap_ipc(
	child: &mut task::TaskCB,
	port_id: u64,
	rights: capability::Rights,
) -> Result<(), &'static str> {
	let boot = task::scheduler::current_task_arc().ok_or("bootstrap: no boot task")?;
	let source = {
		let cspace = boot.lock().cspace.clone();
		let cspace = cspace.lock();
		let store = capability::global_cap_store().lock();
		cspace.iter().find_map(|(_, id, _)| {
			let record = store.lookup(id)?;
			match record.cap_type {
				capability::CapabilityType::IpcPort { port_id: id, .. } if id == port_id => Some(record),
				_ => None,
			}
		})
	}.ok_or("bootstrap: IPC root capability missing")?;
	let child_record = capability::global_cap_store()
		.lock()
		.delegate_checked(source.id, rights, 0, 0)
		.map_err(|_| "bootstrap: unable to delegate IPC capability")?;
	child.cspace.lock().insert(child_record.id, false).ok_or("bootstrap: C-space full")?;
	Ok(())
}
