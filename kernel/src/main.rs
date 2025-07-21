#![no_std]
#![no_main]
#![feature(alloc_error_handler)]

extern crate alloc;
mod util;

use crate::util::HEAP_ALLOCATOR;
use alloc::boxed::Box;
use core::ptr;
use limine::memory_map::{Entry, EntryType};
use limine::request::{FramebufferRequest, MemoryMapRequest};
use limine::BaseRevision;
use x86_64::registers::control::Cr3;
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
};
use x86_64::{PhysAddr, VirtAddr};

const HEAP_START: usize = 0x4444_4444_0000;
const HEAP_SIZE: usize = 1024 * 1024;

static BASE_REVISION: BaseRevision = BaseRevision::new();
static FRAMEBUFFER_REQ: FramebufferRequest = FramebufferRequest::new();
static MMAP_REQ: MemoryMapRequest = MemoryMapRequest::new();

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    //Access framebuffer info
    let fb_response = FRAMEBUFFER_REQ
        .get_response()
        .expect("No framebuffer reply");

    let mmap_response = MMAP_REQ.get_response().expect("No memory map response");
    let entries = mmap_response.entries();

    //Get kernel physical memory offset
    let phys_mem_offset = VirtAddr::new(0xffff_8000_0000_0000);
    let mut mapper = unsafe { init_offset_page_table(phys_mem_offset) };
    //BUG: BootFrameAllocator::new uses heap before heap is initialized
    let mut frame_alloc = BootFrameAllocator::new(mmap_response.entries());

    //--- HEAP MAP/INIT ---
    init_heap(&mut mapper, &mut frame_alloc);
    //Paint screen blue
    if let Some(fb) = fb_response.framebuffers().next() {
        let width = fb.width() as usize;
        let height = fb.height() as usize;
        let pitch = fb.pitch() as usize;
        let bpp = fb.bpp() as usize;

        let ptr = fb.addr() as *mut u8;
        let blue_pixel = [0xFF, 0x00, 0x00, 0x00]; // BGRA, 32 bits

        for y in 0..height {
            for x in 0..width {
                let offset = y * pitch + x * (bpp / 8);
                unsafe {
                    ptr::copy_nonoverlapping(blue_pixel.as_ptr(), ptr.add(offset), 4);
                }
            }
        }

        //Visualize mmap - draw pixel mid-screen with color indicating type
        let count = entries.len();
        let max_count = width.min(count);

        //Thick vertical bar at bottom of screen
        let bar_width = width / max_count.max(1);

        for (i, entry) in entries.iter().enumerate() {
            let color = match entry.entry_type {
                EntryType::USABLE => [0x00, 0xFF, 0x00, 0x00], // green
                EntryType::BOOTLOADER_RECLAIMABLE => [0xFF, 0xFF, 0x00, 0x00], // cyan
                _ => [0x80, 0x80, 0x80, 0x00],                 // gray
            };
            let x_start = i * bar_width;
            for x in x_start..(x_start + bar_width) {
                for y in (height - 40)..height {
                    let offset = y * pitch + x * (bpp / 8);
                    unsafe {
                        ptr::copy_nonoverlapping(color.as_ptr(), ptr.add(offset), 4);
                    }
                }
            }
        }
    }
    loop {}
}

//Returns a mutable reference to the active level-4 page table
unsafe fn active_level_table(offset: VirtAddr) -> &'static mut PageTable {
    let (frame, _) = Cr3::read();
    let phys = frame.start_address().as_u64();
    let virt = offset.as_u64() + phys;
    &mut *(virt as *mut PageTable)
}

unsafe fn init_offset_page_table(offset: VirtAddr) -> OffsetPageTable<'static> {
    OffsetPageTable::new(active_level_table(offset), offset)
}

pub struct BootFrameAllocator {
    frames: &'static [PhysFrame],
    next: usize,
}

impl BootFrameAllocator {
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

pub fn init_heap(
    mapper: &mut OffsetPageTable,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let page_range = {
        let heap_start = VirtAddr::new(HEAP_START as u64);
        let heap_end = VirtAddr::new((HEAP_START + HEAP_SIZE - 1) as u64);
        let start_page = Page::containing_address(heap_start);
        let end_page = Page::containing_address(heap_end);
        Page::range_inclusive(start_page, end_page)
    };

    for page in page_range {
        let frame = frame_allocator
            .allocate_frame()
            .expect("No frames available");
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            mapper
                .map_to(page, frame, flags, frame_allocator)
                .expect("Mapping failed")
                .flush();
        }
    }
    unsafe {
        HEAP_ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }
}
