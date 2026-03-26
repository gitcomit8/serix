/*
 * virtqueue.rs - VirtIO Virtqueue Implementation
 *
 * Provides generic virtqueue data structures and management for VirtIO
 * devices. A virtqueue consists of three DMA-accessible regions:
 * - Descriptor table: array of buffer descriptors
 * - Available ring: driver-to-device buffer indices
 * - Used ring: device-to-driver completion notifications
 *
 * Memory is allocated as physical frames via the page allocator,
 * accessed through HHDM for correct virt→phys DMA translation.
 */

use core::ptr::{read_volatile, write_volatile};
use x86_64::structures::paging::{FrameAllocator, Size4KiB};

/*
 * alloc_dma_page - Allocate a physical frame and return HHDM virtual address
 * @hhdm_offset: HHDM base offset
 *
 * Physical address is trivially virt - hhdm_offset, which is what
 * VirtIO devices need for DMA.
 */
fn alloc_dma_page(hhdm_offset: u64) -> Option<*mut u8> {
	let mut pa = memory::PAGE_ALLOC.get()?.lock();
	let frame = pa.frame_alloc.allocate_frame()?;
	let phys = frame.start_address().as_u64();
	let virt = hhdm_offset + phys;
	unsafe {
		core::ptr::write_bytes(virt as *mut u8, 0, 4096);
	}
	Some(virt as *mut u8)
}

/* Descriptor flags */
pub const VIRTQ_DESC_F_NEXT: u16 = 1;  /* Descriptor is chained */
pub const VIRTQ_DESC_F_WRITE: u16 = 2; /* Device-writable (vs device-readable) */

/*
 * struct VirtqDesc - Virtqueue descriptor table entry
 * @addr:  Physical address of the buffer
 * @len:   Length of the buffer in bytes
 * @flags: NEXT, WRITE, INDIRECT
 * @next:  Index of next descriptor if NEXT flag is set
 */
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtqDesc {
	pub addr: u64,
	pub len: u32,
	pub flags: u16,
	pub next: u16,
}

/*
 * struct VirtqAvail - Available ring (driver → device)
 * @flags: Interrupt suppression flags
 * @idx:   Next index the driver will write to
 *
 * The ring entries follow immediately after this header in memory.
 * ring[i] contains the head descriptor index of a chain.
 */
#[repr(C)]
pub struct VirtqAvail {
	pub flags: u16,
	pub idx: u16,
	/* ring: [u16; queue_size] follows in memory */
}

/*
 * struct VirtqUsedElem - Used ring element
 * @id:  Head descriptor index of the completed chain
 * @len: Total bytes written by the device
 */
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct VirtqUsedElem {
	pub id: u32,
	pub len: u32,
}

/*
 * struct VirtqUsed - Used ring (device → driver)
 * @flags: Notification suppression flags
 * @idx:   Next index the device will write to
 *
 * ring[i] contains a VirtqUsedElem describing a completed chain.
 */
#[repr(C)]
pub struct VirtqUsed {
	pub flags: u16,
	pub idx: u16,
	/* ring: [VirtqUsedElem; queue_size] follows in memory */
}

/*
 * struct Virtqueue - Complete virtqueue state
 * @desc:           Pointer to descriptor table (virtual)
 * @avail:          Pointer to available ring (virtual)
 * @used:           Pointer to used ring (virtual)
 * @desc_phys:      Physical address of descriptor table
 * @avail_phys:     Physical address of available ring
 * @used_phys:      Physical address of used ring
 * @queue_size:     Number of descriptors
 * @free_head:      Head of free descriptor chain
 * @num_free:       Number of available descriptors
 * @last_used_idx:  Last processed used ring index
 */
pub struct Virtqueue {
	desc: *mut VirtqDesc,
	avail: *mut VirtqAvail,
	used: *mut VirtqUsed,
	pub desc_phys: u64,
	pub avail_phys: u64,
	pub used_phys: u64,
	pub queue_size: u16,
	free_head: u16,
	num_free: u16,
	last_used_idx: u16,
}

unsafe impl Send for Virtqueue {}

impl Virtqueue {
	/*
	 * allocate - Allocate and initialize a virtqueue
	 * @queue_size:  Number of descriptors (from device)
	 * @hhdm_offset: Higher Half Direct Map offset for phys addr calculation
	 *
	 * Allocates three 4KiB pages (desc, avail, used), zeroes them, and
	 * initializes the free descriptor chain.
	 *
	 * Return: Some(Virtqueue) on success, None on OOM
	 */
	pub fn allocate(queue_size: u16, hhdm_offset: u64) -> Option<Self> {
		let desc_virt = alloc_dma_page(hhdm_offset)? as u64;
		let avail_virt = alloc_dma_page(hhdm_offset)? as u64;
		let used_virt = alloc_dma_page(hhdm_offset)? as u64;

		let desc_phys = desc_virt - hhdm_offset;
		let avail_phys = avail_virt - hhdm_offset;
		let used_phys = used_virt - hhdm_offset;

		let desc = desc_virt as *mut VirtqDesc;

		/* Initialize free descriptor chain: each points to next */
		for i in 0..queue_size {
			unsafe {
				let d = desc.add(i as usize);
				write_volatile(
					&raw mut (*d).next,
					if i + 1 < queue_size { i + 1 } else { 0 },
				);
				write_volatile(&raw mut (*d).flags, 0);
			}
		}

		Some(Virtqueue {
			desc,
			avail: avail_virt as *mut VirtqAvail,
			used: used_virt as *mut VirtqUsed,
			desc_phys,
			avail_phys,
			used_phys,
			queue_size,
			free_head: 0,
			num_free: queue_size,
			last_used_idx: 0,
		})
	}

	/*
	 * push_chain - Submit a descriptor chain to the device
	 * @descs: Slice of (phys_addr, len, flags) tuples
	 *
	 * Allocates descriptors from the free list, fills them, chains
	 * them via NEXT flags, and adds the head to the available ring.
	 *
	 * Return: Head descriptor index, or None if insufficient free descs
	 */
	pub fn push_chain(&mut self, descs: &[(u64, u32, u16)]) -> Option<u16> {
		if descs.is_empty() || self.num_free < descs.len() as u16 {
			return None;
		}

		let head = self.free_head;
		let mut idx = head;

		for (i, &(addr, len, flags)) in descs.iter().enumerate() {
			let is_last = i == descs.len() - 1;
			unsafe {
				let d = self.desc.add(idx as usize);
				let next = read_volatile(&raw const (*d).next);
				write_volatile(&raw mut (*d).addr, addr);
				write_volatile(&raw mut (*d).len, len);
				if is_last {
					/* Strip NEXT from last descriptor */
					write_volatile(
						&raw mut (*d).flags,
						flags & !VIRTQ_DESC_F_NEXT,
					);
				} else {
					write_volatile(
						&raw mut (*d).flags,
						flags | VIRTQ_DESC_F_NEXT,
					);
				}
				if !is_last {
					idx = next;
				} else {
					self.free_head = next;
				}
			}
			self.num_free -= 1;
		}

		/* Add head to available ring */
		unsafe {
			let avail_idx =
				read_volatile(&raw const (*self.avail).idx);
			let ring_entry = (self.avail as *mut u16)
				.add(2 + (avail_idx % self.queue_size) as usize);
			write_volatile(ring_entry, head);

			/* Memory barrier before updating idx */
			core::sync::atomic::fence(
				core::sync::atomic::Ordering::Release,
			);
			write_volatile(
				&raw mut (*self.avail).idx,
				avail_idx.wrapping_add(1),
			);
		}

		Some(head)
	}

	/*
	 * pop_used - Check for completed descriptor chains
	 *
	 * Return: Some((head_desc_id, bytes_written)) if a chain completed,
	 *         None if no new completions
	 */
	pub fn pop_used(&mut self) -> Option<(u32, u32)> {
		unsafe {
			core::sync::atomic::fence(
				core::sync::atomic::Ordering::Acquire,
			);
			let used_idx =
				read_volatile(&raw const (*self.used).idx);

			if self.last_used_idx == used_idx {
				return None;
			}

			let ring_entry = (self.used as *mut u8)
				.add(4) as *const VirtqUsedElem;
			let elem = ring_entry.add(
				(self.last_used_idx % self.queue_size) as usize,
			);
			let id = read_volatile(&raw const (*elem).id);
			let len = read_volatile(&raw const (*elem).len);

			self.last_used_idx = self.last_used_idx.wrapping_add(1);
			Some((id, len))
		}
	}

	/*
	 * free_chain - Return a descriptor chain to the free list
	 * @head: Head descriptor index of the chain to free
	 *
	 * Walks the chain following NEXT pointers and returns all
	 * descriptors to the free list.
	 */
	pub fn free_chain(&mut self, head: u16) {
		let mut idx = head;
		loop {
			unsafe {
				let d = self.desc.add(idx as usize);
				let flags = read_volatile(&raw const (*d).flags);
				let next = read_volatile(&raw const (*d).next);

				/* Point this desc to current free_head */
				write_volatile(&raw mut (*d).next, self.free_head);
				write_volatile(&raw mut (*d).flags, 0);
				self.free_head = idx;
				self.num_free += 1;

				if flags & VIRTQ_DESC_F_NEXT != 0 {
					idx = next;
				} else {
					break;
				}
			}
		}
	}
}
