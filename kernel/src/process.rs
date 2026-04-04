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
use x86_64::structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB};

/*
 * user_entry_trampoline - Ring 0 → Ring 3 bridge for newly spawned tasks
 *
 * context_switch() ret's here on the task's first scheduled time slice.
 * At entry: kernel stack is valid, r12 = user entry point, r13 = user RSP.
 * Builds the iretq frame and switches to Ring 3.
 *
 * GS is NOT swapped here. At spawn time GS_BASE = 0 (user) and
 * KernelGsBase = PER_CPU_DATA, which is exactly what the syscall trampoline
 * expects. swapgs in the syscall handler will make PER_CPU_DATA active.
 */
#[unsafe(naked)]
pub unsafe extern "C" fn user_entry_trampoline() -> ! {
	naked_asm!(
		"cli",
		"push 0x23",          /* SS  — user data RPL 3 */
		"push r13",           /* RSP — user stack pointer */
		"push 0x202",         /* RFLAGS — IF=1, reserved=1 */
		"push 0x2B",          /* CS  — user code RPL 3 */
		"push r12",           /* RIP — user entry point */
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
	mapper: &mut impl Mapper<Size4KiB>,
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
		let new_frame = allocator.allocate_frame().expect("OOM during segment load");
		let (frame, freshly_mapped) = match mapper.map_to(page, new_frame, flags, allocator) {
			Ok(f) => { f.flush(); (new_frame, true) }
			Err(MapToError::PageAlreadyMapped(existing)) => (existing, false),
			Err(e) => panic!("map_segment: {:?}", e),
		};

		let frame_virt = phys_offset + frame.start_address().as_u64();
		let ptr = frame_virt.as_mut_ptr::<u8>();
		if freshly_mapped {
			ptr.write_bytes(0, 4096);
		}

		let page_addr = page.start_address().as_u64();
		let seg_addr = start.as_u64();

		let data_start = if page_addr < seg_addr { 0 } else { page_addr - seg_addr };
		let data_end = core::cmp::min(
			segment.data.len() as u64,
			(page_addr + 4096).saturating_sub(seg_addr),
		);

		if data_start < data_end {
			let dest_offset = if page_addr < seg_addr { seg_addr - page_addr } else { 0 };
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
	let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::USER_ACCESSIBLE;

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
		if n == 0 { break; }
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
				map_segment(&mut user_mapper, &mut alloc_guard.frame_alloc, seg, phys_offset);
			}
		}

		let ust = unsafe {
			allocate_user_stack(&mut user_mapper, &mut alloc_guard.frame_alloc, phys_offset)
		};

		drop(alloc_guard); /* release before kernel stack alloc */

		/* Allocate kernel stack from dedicated fixed range (always mapped) */
		let ks = memory::kstack::alloc_kernel_stack(1024 * 1024)
			.ok_or("spawn: OOM kernel stack")?;

		(pml4, ust, ks)
	};

	/* 7. Build CPUContext — trampoline runs in Ring 0, then iretq to Ring 3 */
	let mut ctx = task::CPUContext::default();
	ctx.rsp = kstack.as_u64();
	ctx.rip = user_entry_trampoline as u64;
	ctx.r12 = image.entry_point.as_u64();  /* user entry point, read by trampoline */
	ctx.r13 = user_stack_top.as_u64();     /* user RSP, read by trampoline */
	ctx.cr3 = pml4_frame.start_address().as_u64();
	ctx.cs = 0x08; /* kernel CS — trampoline is Ring 0 code */
	ctx.ss = 0x10;
	ctx.rflags = 0x202;

	/* 8. Allocate task ID and build TaskCB */
	let child_id_val = task::TaskId::new();
	let child_id = child_id_val.0;

	let tcb = task::TaskCB {
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
	};

	/* 9. Initialise stdio fds */
	crate::fd::init_stdio(child_id);

	/* 10. Register child in parent's children list */
	if parent_id != 0 {
		if let Some(parent_arc) = task::scheduler::find_task_by_id(parent_id) {
			parent_arc.lock().children.push(child_id);
		}
	}

	/* 11. Enqueue */
	task::scheduler::enqueue_task(Arc::new(Mutex::new(tcb)));

	hal::serial_println!("[SPAWN] pid={} entry={:#x} cr3={:#x}",
		child_id, image.entry_point.as_u64(), pml4_frame.start_address().as_u64());

	Ok(child_id)
}
