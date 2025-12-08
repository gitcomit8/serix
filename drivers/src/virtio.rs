/*
 * VirtIO Block Driver
 *
 * Implements VirtIO 1.0 Block Device driver over PCI/MMIO.
 */

use crate::pci::PciDevice;
use core::ptr::{read_volatile, write_volatile};

/* VirtIO Capability Constants */
const VIRTIO_PCI_CAP_COMMON_CFG: u8 = 1;
const VIRTIO_PCI_CAP_NOTIFY_CFG: u8 = 2;
const VIRTIO_PCI_CAP_ISR_CFG: u8 = 3;
const VIRTIO_PCI_CAP_DEVICE_CFG: u8 = 4;
const VIRTIO_PCI_CAP_PCI_CFG: u8 = 5;

/* Device Status Bits */
const STATUS_ACKNOWLEDGE: u8 = 1;
const STATUS_DRIVER: u8 = 2;
const STATUS_FAILED: u8 = 128;
const STATUS_FEATURES_OK: u8 = 8;
const STATUS_DRIVER_OK: u8 = 4;

/*
 * struct VirtioPciCap - Generic VirtIO Capability Structure
 * Found in PCI configuration space.
 */
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
struct VirtioPciCap {
	cap_vndr: u8, // 0x09
	cap_next: u8,
	cap_len: u8,
	cfg_type: u8, // 1..5
	bar: u8,
	id: u8,
	padding: [u8; 2],
	offset: u32,
	length: u32,
}

/*
 * struct VirtioCommonCfg - Common Configuration Structure
 * Located in MMIO BAR.
 */
#[repr(C)]
struct VirtioCommonCfg {
	device_feature_select: u32, // 0x00
	device_feature: u32,        // 0x04
	driver_feature_select: u32, // 0x08
	driver_feature: u32,        // 0x0C
	msix_config: u16,           // 0x10
	num_queues: u16,            // 0x12
	device_status: u8,          // 0x14
	config_generation: u8,      // 0x15
	queue_select: u16,          // 0x16
	queue_size: u16,            // 0x18
	queue_msix_vector: u16,     // 0x1A
	queue_enable: u16,          // 0x1C
	queue_notify_off: u16,      // 0x1E
	queue_desc_lo: u32,         // 0x20
	queue_desc_hi: u32,         // 0x24
	queue_avail_lo: u32,        // 0x28
	queue_avail_hi: u32,        // 0x2C
	queue_used_lo: u32,         // 0x30
	queue_used_hi: u32,         // 0x34
}

pub struct VirtioBlock {
	common_cfg: *mut VirtioCommonCfg,
}

impl VirtioBlock {
	/*
	 * init - Initialize VirtIO Block Device
	 * @dev: The PCI device instance
	 * @map_mmio: Callback to map physical address to virtual
	 *
	 * Returns an initialized driver instance if successful.
	 */
	pub unsafe fn init<F>(dev: PciDevice, mut map_mmio: F) -> Option<Self>
	where
		F: FnMut(u64, u64) -> *mut u8, // Changed Fn -> FnMut
	{
		// 1. Verify Device ID (Legacy: 0x1001, Modern: 0x1042 for Block)
		// We focus on Modern (1.0+) here.
		if dev.vendor_id != 0x1AF4 || dev.device_id < 0x1040 {
			return None;
		}

		hal::serial_println!("VirtIO: Found potential device (ID: {:x})", dev.device_id);

		// 2. Enable Bus Master
		dev.enable_bus_master();

		// 3. Find Common Configuration Capability
		let mut common_cfg_ptr: Option<*mut VirtioCommonCfg> = None;
		let mut ptr = dev.find_capability(0x09); // Vendor Specific

		while let Some(offset) = ptr {
			// Read the capability structure manually from config space
			let cfg_type = dev.read_u8(offset + 3);
			let bar_idx = dev.read_u8(offset + 4);
			let offset_in_bar = dev.read_u32(offset + 8);
			let length = dev.read_u32(offset + 12);

			if cfg_type == VIRTIO_PCI_CAP_COMMON_CFG {
				// Found it! Get the BAR address.
				if let Some((bar_phys, _)) = dev.get_bar(bar_idx) {
					// Map the MMIO region
					let virt_base = map_mmio(bar_phys + offset_in_bar as u64, length as u64);
					common_cfg_ptr = Some(virt_base as *mut VirtioCommonCfg);
					hal::serial_println!("VirtIO: Mapped Common Cfg at {:#p}", virt_base);
				}
				break;
			}

			// Move to next capability
			let next = dev.read_u8(offset + 1);
			ptr = if next != 0 { Some(next) } else { None };
		}

		let cfg = common_cfg_ptr?;

		// 4. Reset Device
		write_volatile(&mut (*cfg).device_status, 0);

		// 5. Set ACKNOWLEDGE status
		let status = read_volatile(&mut (*cfg).device_status);
		write_volatile(&mut (*cfg).device_status, status | STATUS_ACKNOWLEDGE);

		// 6. Set DRIVER status
		let status = read_volatile(&mut (*cfg).device_status);
		write_volatile(&mut (*cfg).device_status, status | STATUS_DRIVER);

		// 7. Negotiate Features (Simple: Accept what's offered, minus what we don't want)
		// Read device features (Select 0 for first 32 bits)
		write_volatile(&mut (*cfg).device_feature_select, 0);
		let features = read_volatile(&mut (*cfg).device_feature);

		// Write back features (we accept all for now, TODO: filter)
		write_volatile(&mut (*cfg).driver_feature_select, 0);
		write_volatile(&mut (*cfg).driver_feature, features);

		// 8. Set FEATURES_OK
		let status = read_volatile(&mut (*cfg).device_status);
		write_volatile(&mut (*cfg).device_status, status | STATUS_FEATURES_OK);

		// 9. Check if device accepted features
		let new_status = read_volatile(&mut (*cfg).device_status);
		if new_status & STATUS_FEATURES_OK == 0 {
			hal::serial_println!("VirtIO: Feature negotiation failed");
			return None;
		}

		// 10. Set DRIVER_OK (Device is live!)
		write_volatile(&mut (*cfg).device_status, new_status | STATUS_DRIVER_OK);
		hal::serial_println!("VirtIO: Driver active!");

		Some(Self { common_cfg: cfg })
	}
}
