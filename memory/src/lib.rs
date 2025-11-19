/*
 * Memory Management
 *
 * Provides page table initialization and frame allocation for virtual memory.
 */

#![no_std]
extern crate alloc;
pub mod heap;

use alloc::boxed::Box;
use limine::memory_map::Entry;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

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
