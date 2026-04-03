/*
 * Memory Management
 *
 * Provides page table initialization and frame allocation for virtual memory.
 */

#![no_std]
extern crate alloc;
pub mod heap;
pub mod slub;
pub mod kstack;

use alloc::boxed::Box;
use limine::memory_map::Entry;
use spin::{Mutex, Once};
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use crate::heap::StaticBootFrameAllocator;

/*
 * struct PageAllocator - Global page mapping resources
 * @mapper:     Active page table mapper
 * @frame_alloc: Physical frame allocator
 *
 * Stored globally so the SLUB allocator (and future subsystems)
 * can map new virtual pages without threading the mapper through
 * every call site.
 */
pub struct PageAllocator {
	pub mapper: OffsetPageTable<'static>,
	pub frame_alloc: StaticBootFrameAllocator,
}

pub static PAGE_ALLOC: Once<Mutex<PageAllocator>> = Once::new();

static HHDM_OFFSET: Once<VirtAddr> = Once::new();

pub fn set_hhdm_offset(offset: VirtAddr) {
	HHDM_OFFSET.call_once(|| offset);
}

pub fn hhdm_offset() -> VirtAddr {
	*HHDM_OFFSET.get().expect("HHDM offset not set")
}

/*
 * init_page_allocator - Store mapper and frame allocator globally
 * @mapper:     OffsetPageTable for the active address space
 * @frame_alloc: Boot frame allocator with remaining frames
 *
 * Call after all early MMIO mappings are done. Transfers ownership
 * so the SLUB and other subsystems can allocate pages on demand.
 */
pub fn init_page_allocator(
	mapper: OffsetPageTable<'static>,
	frame_alloc: StaticBootFrameAllocator,
) {
	PAGE_ALLOC.call_once(|| Mutex::new(PageAllocator { mapper, frame_alloc }));
}

/*
 * active_level_table - Get mutable reference to active level-4 page table
 * @offset: Higher Half Direct Map offset
 *
 * Returns a mutable reference to the currently active page table.
 */
unsafe fn active_level_table(offset: VirtAddr) -> &'static mut PageTable {
	let (frame, _) = Cr3::read();
	let phys = frame.start_address().as_u64();
	let virt = offset.as_u64() + phys;
	&mut *(virt as *mut PageTable)
}

/*
 * init_offset_page_table - Initialize an OffsetPageTable
 * @offset: Higher Half Direct Map offset
 *
 * Creates an OffsetPageTable for manipulating virtual memory mappings.
 */
pub unsafe fn init_offset_page_table(offset: VirtAddr) -> OffsetPageTable<'static> {
	OffsetPageTable::new(active_level_table(offset), offset)
}

/*
 * struct BootFrameAllocator - Frame allocator using heap-allocated array
 * @frames: Static slice of available physical frames
 * @next: Index of next frame to allocate
 *
 * Allocates physical frames from a pre-populated list.
 */
pub struct BootFrameAllocator {
	frames: &'static [PhysFrame],
	next: usize,
}

impl BootFrameAllocator {
	/*
	 * new - Create a frame allocator from memory map
	 * @memory_map: Array of Limine memory map entries
	 *
	 * Collects all usable physical frames from the memory map.
	 */
	pub fn new(memory_map: &[&Entry]) -> Self {
		let mut frames = alloc::vec::Vec::new();
		for region in memory_map
			.iter()
			.filter(|r| r.entry_type == limine::memory_map::EntryType::USABLE)
		{
			let start = region.base;
			let end = region.base + region.length;
			let start_frame = PhysFrame::containing_address(PhysAddr::new(start));
			let end_frame = PhysFrame::containing_address(PhysAddr::new(end - 1));
			for frame in PhysFrame::range_inclusive(start_frame, end_frame) {
				frames.push(frame);
			}
		}
		let boxed = frames.into_boxed_slice();
		let static_frames = Box::leak(boxed);
		BootFrameAllocator {
			frames: static_frames,
			next: 0,
		}
	}
}

unsafe impl FrameAllocator<Size4KiB> for BootFrameAllocator {
	fn allocate_frame(&mut self) -> Option<PhysFrame> {
		if self.next < self.frames.len() {
			let frame = self.frames[self.next];
			self.next += 1;
			Some(frame)
		} else {
			None
		}
	}
}

pub unsafe fn create_user_page_table(
	frame_allocator: &mut impl FrameAllocator<Size4KiB>,
	offset: VirtAddr,
) -> Option<PhysFrame> {
	let pml4_frame = frame_allocator.allocate_frame()?;
	let pml4_phys = pml4_frame.start_address();
	let pml4_virt = offset + pml4_phys.as_u64();

	let pml4_table = &mut *(pml4_virt.as_mut_ptr() as *mut PageTable);
	pml4_table.zero();

	let (active_frame, _) = Cr3::read();
	let active_phys = active_frame.start_address();
	let active_virt = offset + active_phys.as_u64();
	let active_table = &*(active_virt.as_ptr() as *const PageTable);

	for i in 256..512 {
		pml4_table[i] = active_table[i].clone();
	}
	Some(pml4_frame)
}

/*
 * create_mapper - Create a mapper for a specific page table
 * @pml4_frame: The physical frame of the target PML4
 * @offset: Physical memory offset (HHDM)
 *
 * Allows modifying a page table that isn't currently active (CR3).
 */
pub unsafe fn create_mapper(pml4_frame: PhysFrame, offset: VirtAddr) -> OffsetPageTable<'static> {
	let phys = pml4_frame.start_address().as_u64();
	let virt = offset + phys;
	let table = &mut *(virt.as_mut_ptr() as *mut PageTable);
	OffsetPageTable::new(table, offset)
}
