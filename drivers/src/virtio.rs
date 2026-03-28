/*
 * VirtIO Block Driver
 *
 * Implements VirtIO 1.0 Block Device driver over PCI/MMIO.
 *
 * Initialization is split into two phases:
 * Phase 1 (init): PCI discovery, capability mapping, feature negotiation
 *   Runs early before SLUB allocator is available.
 *   Stops at FEATURES_OK status.
 *
 * Phase 2 (setup_queues): Virtqueue allocation, queue programming, DRIVER_OK
 *   Runs after SLUB is initialized so DMA memory can be allocated.
 */

use crate::pci::PciDevice;
use crate::virtqueue::Virtqueue;
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use spin::{Mutex, Once};
use x86_64::structures::idt::InterruptStackFrame;

/* VirtIO PCI Capability Types */
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;

/* Device Status Bits */
const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_DRIVER_OK: u8 = 4;

/*
 * struct VirtioCommonCfg - Common Configuration Structure
 *
 * Located in MMIO BAR. Provides access to device features,
 * queue configuration, and device status.
 */
#[repr(C)]
struct VirtioCommonCfg {
	device_feature_select: u32, /* 0x00 */
	device_feature: u32,        /* 0x04 */
	driver_feature_select: u32, /* 0x08 */
	driver_feature: u32,        /* 0x0C */
	msix_config: u16,           /* 0x10 */
	num_queues: u16,            /* 0x12 */
	device_status: u8,          /* 0x14 */
	config_generation: u8,      /* 0x15 */
	queue_select: u16,          /* 0x16 */
	queue_size: u16,            /* 0x18 */
	queue_msix_vector: u16,     /* 0x1A */
	queue_enable: u16,          /* 0x1C */
	queue_notify_off: u16,      /* 0x1E */
	queue_desc_lo: u32,         /* 0x20 */
	queue_desc_hi: u32,         /* 0x24 */
	queue_avail_lo: u32,        /* 0x28 */
	queue_avail_hi: u32,        /* 0x2C */
	queue_used_lo: u32,         /* 0x30 */
	queue_used_hi: u32,         /* 0x34 */
}

/*
 * struct VirtioBlock - VirtIO block device driver instance
 * @common_cfg:           Pointer to common configuration registers
 * @notify_cfg_base:      Base address of notify capability region
 * @notify_off_multiplier: Multiplier for queue notification offset
 * @isr_cfg:              Pointer to ISR status register
 * @device_cfg:           Pointer to device-specific configuration
 * @pci_dev:              PCI device info (for interrupt line query)
 * @queue:                Virtqueue (populated in phase 2)
 * @hhdm_offset:          HHDM offset for virt→phys translation
 */
pub struct VirtioBlock {
	common_cfg: *mut VirtioCommonCfg,
	notify_cfg_base: *mut u8,
	notify_off_multiplier: u32,
	isr_cfg: *mut u8,
	device_cfg: *mut u8,
	pub pci_dev: PciDevice,
	queue: Option<Virtqueue>,
	hhdm_offset: u64,
}

/* Raw pointers are MMIO addresses, only accessed under Mutex */
unsafe impl Send for VirtioBlock {}

/* Global VirtIO block device instance */
static VIRTIO_BLK: Once<Mutex<VirtioBlock>> = Once::new();

/* ISR address for interrupt handler (avoids locking the Mutex) */
static ISR_CFG_ADDR: AtomicU64 = AtomicU64::new(0);

/* Set by ISR, polled by I/O functions */
static VIRTIO_BLK_COMPLETE: AtomicBool = AtomicBool::new(false);

const VIRTIO_BLK_VECTOR: u8 = 34;

/*
 * virtio_blk_interrupt_handler - VirtIO block device ISR
 *
 * Reads ISR status register (which acknowledges the interrupt),
 * sets the completion flag, and sends EOI.
 */
extern "x86-interrupt" fn virtio_blk_interrupt_handler(
	_frame: InterruptStackFrame,
) {
	let isr_addr = ISR_CFG_ADDR.load(Ordering::Relaxed);
	if isr_addr != 0 {
		/* Reading ISR status acknowledges the interrupt */
		unsafe {
			read_volatile(isr_addr as *const u8);
		}
	}
	VIRTIO_BLK_COMPLETE.store(true, Ordering::Release);
	unsafe { apic::send_eoi(); }
}

/*
 * register_interrupt - Set up interrupt for VirtIO block device
 *
 * Reads PCI interrupt line, maps IRQ via IOAPIC, registers IDT handler.
 */
pub fn register_interrupt() {
	if let Some(blk) = VIRTIO_BLK.get() {
		let dev = blk.lock();
		let irq = unsafe { dev.pci_dev.interrupt_line() };
		let pin = unsafe { dev.pci_dev.interrupt_pin() };

		if pin == 0 {
			hal::serial_println!("VirtIO: No interrupt pin, staying polled");
			return;
		}

		/* Store ISR address for handler (lock-free) */
		ISR_CFG_ADDR.store(dev.isr_cfg as u64, Ordering::Relaxed);

		unsafe {
			apic::ioapic::map_irq(irq, VIRTIO_BLK_VECTOR);
			idt::register_interrupt_handler(
				VIRTIO_BLK_VECTOR,
				virtio_blk_interrupt_handler,
			);
		}
		hal::serial_println!(
			"VirtIO: IRQ {} → vector {}, interrupt registered",
			irq, VIRTIO_BLK_VECTOR,
		);
	}
}

/*
 * virtio_blk - Get reference to the global VirtIO block device
 *
 * Return: Some(&Mutex<VirtioBlock>) if initialized, None otherwise
 */
pub fn virtio_blk() -> Option<&'static Mutex<VirtioBlock>> {
	VIRTIO_BLK.get()
}

/*
 * store_global - Store a VirtioBlock instance globally
 * @blk: Initialized (phase 1) VirtioBlock
 */
pub fn store_global(blk: VirtioBlock) {
	VIRTIO_BLK.call_once(|| Mutex::new(blk));
}

/*
 * setup_queues_global - Complete phase 2 init on the global device
 *
 * Must be called after SLUB allocator is initialized.
 */
pub fn setup_queues_global() {
	if let Some(blk) = VIRTIO_BLK.get() {
		let mut dev = blk.lock();
		if unsafe { dev.setup_queues() } {
			hal::serial_println!("VirtIO: Queues configured, device live");
		} else {
			hal::serial_println!("VirtIO: Queue setup failed");
		}
	}
}

impl VirtioBlock {
	/*
	 * init - Phase 1: PCI discovery, capability mapping, feature negotiation
	 * @dev:         PCI device to initialize
	 * @map_mmio:    Callback to map physical MMIO to virtual
	 * @hhdm_offset: HHDM offset for physical address computation
	 *
	 * Walks all VirtIO PCI capabilities, maps MMIO regions, negotiates
	 * features, and stops at FEATURES_OK. Does NOT set up virtqueues
	 * or set DRIVER_OK — that happens in setup_queues().
	 *
	 * Return: Some(VirtioBlock) on success, None on failure
	 */
	pub unsafe fn init<F>(
		dev: PciDevice,
		mut map_mmio: F,
		hhdm_offset: u64,
	) -> Option<Self>
	where
		F: FnMut(u64, u64) -> *mut u8,
	{
		if dev.vendor_id != 0x1AF4 || dev.device_id < 0x1040 {
			return None;
		}

		hal::serial_println!(
			"VirtIO: Found device (ID: {:#x})", dev.device_id
		);
		dev.enable_bus_master();

		/* Walk all vendor-specific capabilities */
		let mut common_cfg_ptr: Option<*mut VirtioCommonCfg> = None;
		let mut notify_cfg_base: *mut u8 = core::ptr::null_mut();
		let mut notify_off_multiplier: u32 = 0;
		let mut isr_cfg: *mut u8 = core::ptr::null_mut();
		let mut device_cfg: *mut u8 = core::ptr::null_mut();

		let mut ptr = dev.find_capability(0x09);
		while let Some(offset) = ptr {
			let cfg_type = dev.read_u8(offset + 3);
			let bar_idx = dev.read_u8(offset + 4);
			let offset_in_bar = dev.read_u32(offset + 8);
			let length = dev.read_u32(offset + 12);

			if let Some((bar_phys, _)) = dev.get_bar(bar_idx) {
				let base = map_mmio(
					bar_phys + offset_in_bar as u64,
					length as u64,
				);
				match cfg_type {
					VIRTIO_PCI_CAP_COMMON_CFG => {
						common_cfg_ptr =
							Some(base as *mut VirtioCommonCfg);
						hal::serial_println!(
							"VirtIO: Common Cfg at {:#p}", base
						);
					}
					VIRTIO_PCI_CAP_NOTIFY_CFG => {
						notify_cfg_base = base;
						/* Multiplier at cap offset + 16 */
						notify_off_multiplier =
							dev.read_u32(offset + 16);
						hal::serial_println!(
							"VirtIO: Notify Cfg at {:#p}, mult={}",
							base, notify_off_multiplier
						);
					}
					VIRTIO_PCI_CAP_ISR_CFG => {
						isr_cfg = base;
					}
					VIRTIO_PCI_CAP_DEVICE_CFG => {
						device_cfg = base;
					}
					_ => {}
				}
			}

			let next = dev.read_u8(offset + 1);
			ptr = if next != 0 { Some(next) } else { None };
		}

		let cfg = common_cfg_ptr?;

		/* Reset device */
		write_volatile(&raw mut (*cfg).device_status, 0);

		/* ACKNOWLEDGE */
		let s = read_volatile(&raw const (*cfg).device_status);
		write_volatile(&raw mut (*cfg).device_status, s | STATUS_ACKNOWLEDGE);

		/* DRIVER */
		let s = read_volatile(&raw const (*cfg).device_status);
		write_volatile(&raw mut (*cfg).device_status, s | STATUS_DRIVER);

		/* Negotiate features: accept all from device word 0 */
		write_volatile(&raw mut (*cfg).device_feature_select, 0);
		let features = read_volatile(&raw const (*cfg).device_feature);
		write_volatile(&raw mut (*cfg).driver_feature_select, 0);
		write_volatile(&raw mut (*cfg).driver_feature, features);

		/* Negotiate VIRTIO_F_VERSION_1 (bit 32 = word 1, bit 0) */
		write_volatile(&raw mut (*cfg).device_feature_select, 1);
		let features_hi = read_volatile(&raw const (*cfg).device_feature);
		write_volatile(&raw mut (*cfg).driver_feature_select, 1);
		write_volatile(
			&raw mut (*cfg).driver_feature,
			features_hi & 0x1, /* Accept only VERSION_1 */
		);

		/* FEATURES_OK */
		let s = read_volatile(&raw const (*cfg).device_status);
		write_volatile(&raw mut (*cfg).device_status, s | STATUS_FEATURES_OK);

		let s = read_volatile(&raw const (*cfg).device_status);
		if s & STATUS_FEATURES_OK == 0 {
			hal::serial_println!("VirtIO: Feature negotiation failed");
			return None;
		}

		hal::serial_println!("VirtIO: Phase 1 complete (FEATURES_OK)");

		Some(Self {
			common_cfg: cfg,
			notify_cfg_base,
			notify_off_multiplier,
			isr_cfg,
			device_cfg,
			pci_dev: dev,
			queue: None,
			hhdm_offset,
		})
	}

	/*
	 * setup_queues - Phase 2: Allocate virtqueues and set DRIVER_OK
	 *
	 * Must be called after SLUB allocator is initialized.
	 * Allocates virtqueue 0 (the requestq for block devices),
	 * programs queue addresses into the device, and transitions
	 * to DRIVER_OK.
	 *
	 * Return: true on success, false on failure
	 */
	pub unsafe fn setup_queues(&mut self) -> bool {
		let cfg = self.common_cfg;

		/* Select queue 0 */
		write_volatile(&raw mut (*cfg).queue_select, 0);
		let queue_size =
			read_volatile(&raw const (*cfg).queue_size);

		if queue_size == 0 {
			hal::serial_println!("VirtIO: Queue size is 0");
			return false;
		}
		hal::serial_println!(
			"VirtIO: Queue 0 size = {}", queue_size
		);

		/* Allocate virtqueue via SLUB */
		let vq = match Virtqueue::allocate(
			queue_size, self.hhdm_offset
		) {
			Some(vq) => vq,
			None => {
				hal::serial_println!("VirtIO: Queue alloc failed");
				return false;
			}
		};

		/* Program queue addresses into device */
		write_volatile(
			&raw mut (*cfg).queue_desc_lo,
			vq.desc_phys as u32,
		);
		write_volatile(
			&raw mut (*cfg).queue_desc_hi,
			(vq.desc_phys >> 32) as u32,
		);
		write_volatile(
			&raw mut (*cfg).queue_avail_lo,
			vq.avail_phys as u32,
		);
		write_volatile(
			&raw mut (*cfg).queue_avail_hi,
			(vq.avail_phys >> 32) as u32,
		);
		write_volatile(
			&raw mut (*cfg).queue_used_lo,
			vq.used_phys as u32,
		);
		write_volatile(
			&raw mut (*cfg).queue_used_hi,
			(vq.used_phys >> 32) as u32,
		);

		/* Read notify offset for this queue */
		let _queue_notify_off =
			read_volatile(&raw const (*cfg).queue_notify_off);

		/* Enable the queue */
		write_volatile(&raw mut (*cfg).queue_enable, 1);

		self.queue = Some(vq);

		/* Set DRIVER_OK — device is now live */
		let s = read_volatile(&raw const (*cfg).device_status);
		write_volatile(
			&raw mut (*cfg).device_status,
			s | STATUS_DRIVER_OK,
		);

		hal::serial_println!("VirtIO: DRIVER_OK — device live");
		true
	}

	/*
	 * notify_queue - Notify the device that queue 0 has new buffers
	 */
	unsafe fn notify_queue(
		common_cfg: *mut VirtioCommonCfg,
		notify_cfg_base: *mut u8,
		notify_off_multiplier: u32,
	) {
		let queue_notify_off =
			read_volatile(&raw const (*common_cfg).queue_notify_off)
				as u32;
		let notify_addr = notify_cfg_base.add(
			(queue_notify_off * notify_off_multiplier) as usize,
		) as *mut u16;
		write_volatile(notify_addr, 0);
	}

	/*
	 * read_sector - Read a 512-byte sector from the block device
	 * @sector: Sector number to read
	 * @buf:    Buffer to receive data
	 *
	 * Submits a 3-descriptor chain (header→data→status) and waits
	 * for interrupt-driven completion via hlt.
	 *
	 * Return: Ok(()) on success, Err(BlockError) on failure
	 */
	pub fn read_sector(
		&mut self,
		sector: u64,
		buf: &mut [u8; 512],
	) -> Result<(), BlockError> {
		let vq = self.queue.as_mut().ok_or(BlockError::IoError)?;

		/* Allocate DMA buffer via physical frame (HHDM-mapped) */
		let dma = alloc_dma_page(self.hhdm_offset)
			.ok_or(BlockError::IoError)? as *mut BlkDmaBuffer;

		unsafe {
			(*dma).header.type_ = VIRTIO_BLK_T_IN;
			(*dma).header.reserved = 0;
			(*dma).header.sector = sector;
			(*dma).status = 0xFF; /* Sentinel */

			let base_phys = dma as u64 - self.hhdm_offset;
			let hdr_phys = base_phys;
			let data_phys = base_phys
				+ core::mem::offset_of!(BlkDmaBuffer, data) as u64;
			let status_phys = base_phys
				+ core::mem::offset_of!(BlkDmaBuffer, status) as u64;

			let chain = [
				(hdr_phys, 16, 0u16), /* header: device-readable */
				(data_phys, 512,
					crate::virtqueue::VIRTQ_DESC_F_WRITE),
				(status_phys, 1,
					crate::virtqueue::VIRTQ_DESC_F_WRITE),
			];

			let head = vq.push_chain(&chain)
				.ok_or(BlockError::IoError)?;

			/* Notify device */
			core::sync::atomic::fence(
				core::sync::atomic::Ordering::SeqCst,
			);
			Self::notify_queue(
				self.common_cfg,
				self.notify_cfg_base,
				self.notify_off_multiplier,
			);

			/* Wait for completion (polled) */
			loop {
				if let Some(_) = vq.pop_used() {
					break;
				}
				core::hint::spin_loop();
			}

			/* Check status */
			let status = read_volatile(&raw const (*dma).status);
			if status != VIRTIO_BLK_S_OK {
				vq.free_chain(head);
				return Err(BlockError::IoError);
			}

			/* Copy data to caller buffer */
			core::ptr::copy_nonoverlapping(
				(*dma).data.as_ptr(), buf.as_mut_ptr(), 512,
			);

			vq.free_chain(head);
		}

		Ok(())
	}

	/*
	 * write_sector - Write a 512-byte sector to the block device
	 * @sector: Sector number to write
	 * @buf:    Buffer containing data to write
	 *
	 * Return: Ok(()) on success, Err(BlockError) on failure
	 */
	pub fn write_sector(
		&mut self,
		sector: u64,
		buf: &[u8; 512],
	) -> Result<(), BlockError> {
		let vq = self.queue.as_mut().ok_or(BlockError::IoError)?;

		/* Allocate DMA buffer via physical frame (HHDM-mapped) */
		let dma = alloc_dma_page(self.hhdm_offset)
			.ok_or(BlockError::IoError)? as *mut BlkDmaBuffer;

		unsafe {
			(*dma).header.type_ = VIRTIO_BLK_T_OUT;
			(*dma).header.reserved = 0;
			(*dma).header.sector = sector;
			(*dma).status = 0xFF;

			/* Copy data into DMA buffer */
			core::ptr::copy_nonoverlapping(
				buf.as_ptr(), (*dma).data.as_mut_ptr(), 512,
			);

			let base_phys = dma as u64 - self.hhdm_offset;
			let hdr_phys = base_phys;
			let data_phys = base_phys
				+ core::mem::offset_of!(BlkDmaBuffer, data) as u64;
			let status_phys = base_phys
				+ core::mem::offset_of!(BlkDmaBuffer, status) as u64;

			let chain = [
				(hdr_phys, 16, 0u16), /* header: device-readable */
				(data_phys, 512, 0u16), /* data: device-readable */
				(status_phys, 1,
					crate::virtqueue::VIRTQ_DESC_F_WRITE),
			];

			let head = vq.push_chain(&chain)
				.ok_or(BlockError::IoError)?;

			core::sync::atomic::fence(
				core::sync::atomic::Ordering::SeqCst,
			);
			Self::notify_queue(
				self.common_cfg,
				self.notify_cfg_base,
				self.notify_off_multiplier,
			);

			loop {
				if let Some(_) = vq.pop_used() {
					break;
				}
				core::hint::spin_loop();
			}

			let status = read_volatile(&raw const (*dma).status);
			vq.free_chain(head);

			if status != VIRTIO_BLK_S_OK {
				return Err(BlockError::IoError);
			}
		}

		Ok(())
	}

	/*
	 * capacity - Get disk capacity in sectors
	 *
	 * Reads the capacity field from device-specific configuration.
	 *
	 * Return: Number of 512-byte sectors
	 */
	pub fn capacity(&self) -> u64 {
		if self.device_cfg.is_null() {
			return 0;
		}
		unsafe {
			read_volatile(self.device_cfg as *const u64)
		}
	}
}

/*
 * Block device request header
 */
#[repr(C)]
struct VirtioBlkReq {
	type_: u32,
	reserved: u32,
	sector: u64,
}

const VIRTIO_BLK_T_IN: u32 = 0;  /* Read */
const VIRTIO_BLK_T_OUT: u32 = 1; /* Write */
const VIRTIO_BLK_S_OK: u8 = 0;

/*
 * struct BlkDmaBuffer - Combined DMA buffer for a single request
 * @header: Request header (16 bytes)
 * @data:   Sector data (512 bytes)
 * @status: Completion status (1 byte)
 *
 * Must be allocated via alloc_dma_page() so the physical address
 * is computable via HHDM for device DMA access.
 */
#[repr(C)]
struct BlkDmaBuffer {
	header: VirtioBlkReq,
	data: [u8; 512],
	status: u8,
}

/*
 * enum BlockError - Block I/O error types
 */
#[derive(Debug)]
pub enum BlockError {
	IoError,
	Unsupported,
}

/*
 * alloc_dma_page - Allocate a physical frame for DMA and return HHDM pointer
 * @hhdm_offset: HHDM base offset
 *
 * Allocates a single physical 4KiB frame from the global page allocator
 * and returns the HHDM virtual address. Physical address is trivially
 * virt - hhdm_offset.
 *
 * Return: Some(virtual_ptr) or None on OOM
 */
fn alloc_dma_page(hhdm_offset: u64) -> Option<*mut u8> {
	use x86_64::structures::paging::{FrameAllocator, Size4KiB};
	let mut pa = memory::PAGE_ALLOC.get()?.lock();
	let frame = pa.frame_alloc.allocate_frame()?;
	let phys = frame.start_address().as_u64();
	let virt = hhdm_offset + phys;
	/* Zero the page */
	unsafe {
		core::ptr::write_bytes(virt as *mut u8, 0, 4096);
	}
	Some(virt as *mut u8)
}
