/*
 * init/process.rs - Process & Memory Mapping Helpers
 *
 * Contains:
 * - enter_user_mode: Ring 0 → Ring 3 transition (IRETQ)
 * - map_segment: Map one ELF PT_LOAD segment into a user address space
 * - allocate_user_stack: Map a 16 KiB user stack near the top of the lower half
 * - init_boot_task: Create the boot task, IPC ports, and seed the run queue
 */

extern crate alloc;

use alloc::sync::Arc;
use hal::serial_println;
use loader::LoadableSegment;
use x86_64::structures::paging::{
	FrameAllocator, Mapper, Page, PageTableFlags, PhysFrame, Size4KiB, Translate,
};
use x86_64::VirtAddr;

/* ------------------------------------------------------------------ */
/*  Ring 0 → Ring 3 transition                                          */
/* ------------------------------------------------------------------ */

/*
 * enter_user_mode - Jump to Ring 3
 * @entry_point: Virtual address of the user program entry
 * @stack_pointer: Virtual address of the user stack top
 * @user_pml4: Physical frame of the user PML4 (for CR3 switch)
 *
 * Performs the IRETQ dance to switch privilege levels.
 * DOES NOT RETURN.
 */
pub unsafe fn enter_user_mode(
	entry_point: VirtAddr,
	stack_pointer: VirtAddr,
	user_pml4: PhysFrame,
) -> ! {
	use x86_64::registers::rflags::RFlags;

	let selectors = crate::gdt::descriptors();
	let rflags = RFlags::INTERRUPT_FLAG.bits();

	let user_ss = selectors.user_data.0 as u64;
	let user_cs = selectors.user_code.0 as u64;

	core::arch::asm!(
		"mov cr3, {cr3}",
		"push {user_ss}",
		"push {rsp}",
		"push {rflags}",
		"push {user_cs}",
		"push {rip}",
		"iretq",

		cr3 = in(reg) user_pml4.start_address().as_u64(),
		user_ss = in(reg) user_ss,
		rsp = in(reg) stack_pointer.as_u64(),
		rflags = in(reg) rflags,
		user_cs = in(reg) user_cs,
		rip = in(reg) entry_point.as_u64(),
		options(noreturn)
	)
}

/* ------------------------------------------------------------------ */
/*  ELF segment mapping helpers                                         */
/* ------------------------------------------------------------------ */

/*
 * map_segment - Map one ELF PT_LOAD segment into a user address space
 * @mapper:        Mapper for the target PML4
 * @allocator:     Physical frame allocator
 * @segment:       Segment descriptor from the loader
 * @phys_offset:   HHDM offset for accessing frame contents
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

		let data_start = if page_addr < seg_addr { 0 } else { page_addr - seg_addr };
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
 * @mapper:        Mapper for the target address space
 * @allocator:     Physical frame allocator
 * @phys_offset:   HHDM offset
 *
 * Returns: Virtual address of the stack top (initial user RSP)
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

/* ------------------------------------------------------------------ */
/*  Boot task initialization                                            */
/* ------------------------------------------------------------------ */

/*
 * init_boot_task - Create the boot task, IPC ports, and seed the run queue
 *
 * The boot task is a placeholder that holds the initial "current" context.
 * The first context switch saves _start's context into boot_task (never
 * re-enqueued), then jumps to the first scheduled task.
 *
 * Also creates IPC ports for the ext4 daemon so the stub can send before
 * the daemon is scheduled.
 *
 * Returns: Arc<Mutex<TaskCB>> for the boot task
 */
pub fn init_boot_task() -> Arc<spin::Mutex<task::TaskCB>> {
	/* Initialize kernel stack allocator before creating user page tables */
	::memory::kstack::init_kstack_region().expect("Failed to init kstack region");
	serial_println!("Kernel stack region initialized");

	/* Seed RunQueue with a boot placeholder as "current" */
	let boot_task = Arc::new(spin::Mutex::new(task::TaskCB::running_task()));

	/* Set boot task as current so create_port inserts into its cspace */
	task::scheduler::get_per_cpu_run_queue(0).lock().current = Some(Arc::clone(&boot_task));

	/* Pre-create IPC ports for ext4d (auto-inserts into boot task's cspace) */
	let (ext4_req_port, _ext4_req_cap) =
		ipc::IPC_GLOBAL.create_port(fs::ext4::ipc::EXT4_REQ_PORT);
	let (ext4_reply_port, _ext4_reply_cap) =
		ipc::IPC_GLOBAL.create_port(fs::ext4::ipc::EXT4_REPLY_BASE);

	serial_println!(
		"ext4d: IPC ports created (req={}, reply_base={})",
		ext4_req_port.id(),
		ext4_reply_port.id()
	);

	/*
	 * ponytail: capabilities are auto-inserted into the current task's
	 * cspace by create_port(). The old push() calls are removed.
	 */

	boot_task
}
