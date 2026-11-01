/*
 * vma.rs - Virtual Memory Area (VMA) management
 *
 * VMAs track mapped regions in userspace:
 *   - Start/end virtual addresses
 *   - Permissions (read/write/exec)
 *   - Backing inode (for file-backed mappings)
 *   - File offset (for file-backed mappings)
 *
 * Used by mmap() to create file-backed memory mappings.
 */

extern crate alloc;
use alloc::collections::BTreeMap;
use alloc::sync::Arc;
use spin::Mutex;
use x86_64::structures::paging::{FrameAllocator, Mapper, PageTableFlags as Flags, Size4KiB};
use x86_64::VirtAddr;

use crate::heap::{HEAP_ALLOCATOR, StaticBootFrameAllocator};
use crate::PAGE_ALLOC;
use crate::PAGE_SIZE;
use crate::hhdm_offset;

use vfs::INode;

/* ------------------------------------------------------------------ */
/*  VMA permissions                                                     */
/* ------------------------------------------------------------------ */

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VmaPermissions {
	pub read: bool,
	pub write: bool,
	pub exec: bool,
}

impl VmaPermissions {
	pub const fn new(read: bool, write: bool, exec: bool) -> Self {
		Self { read, write, exec }
	}

	pub fn allows_read(&self) -> bool {
		self.read
	}

	pub fn allows_write(&self) -> bool {
		self.write
	}

	pub fn allows_exec(&self) -> bool {
		self.exec
	}
}

/* ------------------------------------------------------------------ */
/*  Mapping type                                                        */
/* ------------------------------------------------------------------ */

/*
 * enum MappingType - How pages are handled when mapped
 *
 * MAP_PRIVATE: Copy-on-write. Writes create a private anonymous page.
 * The original file-backed page is never modified.
 * MAP_SHARED: Write-through. Pages are mapped read-write and dirty
 * pages are tracked in the page cache for later writeback.
 */
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MappingType {
	Private,  /* MAP_PRIVATE — CoW */
	Shared,   /* MAP_SHARED — write-through */
}

/* ------------------------------------------------------------------ */
/*  VMA struct                                                          */
/* ------------------------------------------------------------------ */

/*
 * struct Vma - Virtual Memory Area
 *
 * Represents a contiguous region of virtual memory.
 * For file-backed mappings, tracks the file offset and backing inode.
 */
#[derive(Clone)]
pub struct Vma {
	/* Start address (inclusive) */
	pub start: VirtAddr,
	/* End address (exclusive) */
	pub end: VirtAddr,
	/* Permissions */
	pub perms: VmaPermissions,
	/* Mapping type: CoW (Private) or write-through (Shared) */
	pub mapping_type: MappingType,
	/* File offset for file-backed mappings */
	pub file_offset: u64,
	/* Arc<dyn INode> pointer of the backing file (for munmap identity check) */
	pub cap_ptr: usize,
	/* Backing inode for CoW — None for anonymous mappings */
	pub backing_inode: Option<Arc<dyn vfs::INode>>,
}

impl Vma {
	pub fn new(
		start: VirtAddr,
		end: VirtAddr,
		perms: VmaPermissions,
		mapping_type: MappingType,
		file_offset: u64,
		cap_ptr: usize,
		backing_inode: Option<Arc<dyn vfs::INode>>,
	) -> Self {
		Self {
			start,
			end,
			perms,
			mapping_type,
			file_offset,
			cap_ptr,
			backing_inode,
		}
	}

	pub fn size(&self) -> usize {
		(self.end.as_u64() - self.start.as_u64()) as usize
	}

	pub fn contains(&self, addr: VirtAddr) -> bool {
		addr >= self.start && addr < self.end
	}
}

/* ------------------------------------------------------------------ */
/*  VMA Manager                                                         */
/* ------------------------------------------------------------------ */

/*
 * struct VmaManager - Manages VMAs for a process
 *
 * Uses a BTreeMap keyed by start address for efficient lookup.
 */
pub struct VmaManager {
	pub maps: Mutex<BTreeMap<u64, Vma>>,
}

impl VmaManager {
	pub fn new() -> Self {
		Self {
			maps: Mutex::new(BTreeMap::new()),
		}
	}

	/*
	 * add_vma - Add a VMA to the manager
	 */
	pub fn add_vma(&self, vma: Vma) {
		let mut maps = self.maps.lock();
		maps.insert(vma.start.as_u64(), vma);
	}

	/*
	 * find_vma - Find the VMA containing the given address
	 */
	pub fn find_vma(&self, addr: VirtAddr) -> Option<Vma> {
		let maps = self.maps.lock();
		maps.range(..addr.as_u64() + 1).next_back().and_then(|(_, vma)| {
			if vma.contains(addr) {
				Some(vma.clone())
			} else {
				None
			}
		})
	}

	/*
	 * remove_vma - Remove a VMA by start address
	 */
	pub fn remove_vma(&self, start: u64) -> Option<Vma> {
		let mut maps = self.maps.lock();
		maps.remove(&start)
	}

	/*
	 * count - Number of VMAs
	 */
	pub fn count(&self) -> usize {
		self.maps.lock().len()
	}
}

/* ------------------------------------------------------------------ */
/*  Global VMA manager                                                  */
/* ------------------------------------------------------------------ */

/*
 * Note: In a full implementation, each process would have its own VMA manager.
 * For now, we use a single global manager for simplicity.
 */
static mut VMA_MANAGER_INSTANCE: Option<VmaManager> = None;

/*
 * init_vma_manager - Initialize the global VMA manager
 */
pub unsafe fn init_vma_manager() {
	VMA_MANAGER_INSTANCE = Some(VmaManager::new());
}

/*
 * get_vma_manager - Get a reference to the global VMA manager
 */
pub fn get_vma_manager() -> Option<&'static VmaManager> {
	unsafe {
		let ptr = core::ptr::addr_of!(VMA_MANAGER_INSTANCE) as *const Option<VmaManager>;
		(*ptr).as_ref()
	}
}

/* ------------------------------------------------------------------ */
/*  mmap helpers                                                        */
/* ------------------------------------------------------------------ */

/*
 * allocate_vma_region - Find a free region in userspace for a VMA
 *
 * Userspace is 0x0000_0000_0000_0000 to 0x0000_8000_0000_0000 (128 TiB).
 * We allocate from the bottom up, aligned to 4 KiB.
 *
 * Uses the given VMA manager to find gaps.
 *
 * Returns the start address, or None if no space available.
 */
pub fn allocate_vma_region(manager: &VmaManager, size: usize) -> Option<VirtAddr> {
	/* Align size to 4 KiB */
	let aligned_size = (size + 0xFFF) & !0xFFF;

	/* Userspace start */
	let start = VirtAddr::new(0x0000_0000_0010_0000); /* 1 MiB, leave room for binary */
	let end = VirtAddr::new(0x0000_7FFF_FFFF_F000); /* 128 TiB - page */

	/* Find first gap that fits */
	let mut next_free = start.as_u64();

	let maps = manager.maps.lock();
	for (_, vma) in maps.iter() {
		if vma.start.as_u64() > next_free {
			/* Gap between next_free and vma.start */
			let gap = vma.start.as_u64() - next_free;
			if gap >= aligned_size as u64 {
				return Some(VirtAddr::new(next_free));
			}
		}
		if vma.end.as_u64() > next_free {
			next_free = vma.end.as_u64();
		}
	}

	/* Check if there's space at the end */
	if end.as_u64() - next_free >= aligned_size as u64 {
		Some(VirtAddr::new(next_free))
	} else {
		None
	}
}

/*
 * map_file_page - Map a single file page into a VMA
 *
 * Maps a pre-read page of file data into the page table at the given
 * virtual address. The caller is responsible for reading the file data.
 */
pub fn map_file_page(
	vaddr: VirtAddr,
	page_data: &[u8; crate::PAGE_SIZE],
	perms: VmaPermissions,
) -> Result<(), &'static str> {
	use x86_64::structures::paging::{Page, PageTableFlags as Flags};

	/* Allocate a physical frame */
	let page_alloc = PAGE_ALLOC.get().ok_or("page allocator not initialized")?;
	let frame = {
		let mut alloc = page_alloc.lock();
		alloc
			.frame_alloc
			.allocate_frame()
			.ok_or("no free frames")?
	};

	/* Build page table flags */
	let mut flags = Flags::PRESENT;
	if perms.allows_write() {
		flags |= Flags::WRITABLE;
	}
	flags |= Flags::USER_ACCESSIBLE;

	/* Map the page */
	let page = Page::<x86_64::structures::paging::Size4KiB>::containing_address(vaddr);
	let mut alloc = page_alloc.lock();
	/* Use raw pointers to avoid double mutable borrow of `alloc` */
	let mapper_ptr = &mut alloc.mapper as *mut x86_64::structures::paging::OffsetPageTable<'static>;
	let frame_alloc_ptr = &mut alloc.frame_alloc as *mut StaticBootFrameAllocator;
	unsafe {
		(*mapper_ptr)
			.map_to(page, frame, flags, &mut *frame_alloc_ptr)
			.map_err(|_| "failed to map page")?;
	}

	/* Copy data into the frame */
	let hhdm = hhdm_offset();
	let frame_virt = hhdm + frame.start_address().as_u64();
	let frame_ptr = frame_virt.as_mut_ptr::<u8>();
	unsafe {
		core::ptr::copy_nonoverlapping(page_data.as_ptr(), frame_ptr, crate::PAGE_SIZE);
	}

	Ok(())
}

/*
 * page_fault_handler - Handle page faults for mmap'd pages
 *
 * When a page fault occurs on a VMA-backed address, this function
 * handles demand paging and copy-on-write:
 *
 * - Shared mapping + read fault: demand-load page from backing inode
 * - Shared mapping + write fault: map writable (page should already exist
 *   from upfront SYS_MMAP mapping)
 * - Private mapping + read fault: demand-load page as read-only
 * - Private mapping + write fault: CoW — allocate private frame, copy
 *   original page data, mark writable
 *
 * The vma_mgr parameter must be the caller's locked reference to the
 * VMA manager. This avoids a cycle between memory and task crates.
 */
pub fn handle_page_fault(
	vma_mgr: &VmaManager,
	addr: VirtAddr,
	write: bool,
) -> Result<(), &'static str> {
	let vma = vma_mgr
		.find_vma(addr)
		.ok_or("no VMA for faulting address")?;

	/* Check permissions */
	if write && !vma.perms.allows_write() {
		return Err("write to read-only mapping");
	}

	/* CoW path: private mapping on write */
	if vma.mapping_type == MappingType::Private && write {
		return cow_fault(&vma, addr);
	}

	/* Shared or private-read: demand-load from backing inode */
	let backing = vma
		.backing_inode
		.as_ref()
		.ok_or("no backing inode for demand load")?;
	let page_offset_in_file = (addr.as_u64() - vma.start.as_u64() + vma.file_offset) as usize;
	let mut page_data = [0u8; crate::PAGE_SIZE];
	let n = backing.read(page_offset_in_file, &mut page_data);
	if n == 0 {
		return Err("EOF during demand load");
	}
	map_file_page(addr, &page_data, vma.perms)
}

/*
 * cow_fault - Copy-on-write page fault handler
 *
 * On write fault for a MAP_PRIVATE mapping, allocate a new private frame,
 * read original page data from the backing inode, and map the new frame
 * writable.
 */
fn cow_fault(vma: &Vma, addr: VirtAddr) -> Result<(), &'static str> {
	/* Allocate a new private frame */
	let page_alloc = PAGE_ALLOC.get().ok_or("page allocator not initialized")?;
	let frame = {
		let mut alloc = page_alloc.lock();
		alloc
			.frame_alloc
			.allocate_frame()
			.ok_or("no free frames")?
	};

	/* Read original page data from backing inode */
	let backing = vma
		.backing_inode
		.as_ref()
		.ok_or("no backing inode for CoW")?;
	let page_offset_in_file = (addr.as_u64() - vma.start.as_u64() + vma.file_offset) as usize;
	let mut page_data = [0u8; crate::PAGE_SIZE];
	let n = backing.read(page_offset_in_file, &mut page_data);
	if n == 0 {
		return Err("EOF during CoW fault");
	}

	/* Map the new frame writable */
	let page = x86_64::structures::paging::Page::<Size4KiB>::containing_address(addr);
	let page_alloc = PAGE_ALLOC.get().ok_or("page allocator not initialized")?;
	let mut alloc = page_alloc.lock();
	let mut flags = Flags::PRESENT | Flags::WRITABLE | Flags::USER_ACCESSIBLE;

	let mapper_ptr = &mut alloc.mapper as *mut x86_64::structures::paging::OffsetPageTable<'static>;
	let frame_alloc_ptr = &mut alloc.frame_alloc as *mut StaticBootFrameAllocator;
	unsafe {
		(*mapper_ptr)
			.map_to(page, frame, flags, &mut *frame_alloc_ptr)
			.map_err(|_| "failed to map CoW page")?;
	}

	/* Copy page data into the new frame */
	let hhdm = hhdm_offset();
	let frame_virt = hhdm + frame.start_address().as_u64();
	let frame_ptr = frame_virt.as_mut_ptr::<u8>();
	unsafe {
		core::ptr::copy_nonoverlapping(page_data.as_ptr(), frame_ptr, crate::PAGE_SIZE);
	}

	Ok(())
}
