/*
 * PCI Bus Driver
 *
 * Lists the PCI bus, handles configuration space access,
 * and provides utilities for device discovery and BAR mapping.
 */

extern crate alloc;
use alloc::vec::Vec;
use hal::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

/* PCI Configuration Register Offsets */
const REG_VENDOR_ID: u8 = 0x00;
const REG_COMMAND: u8 = 0x04;
const REG_HEADER_TYPE: u8 = 0x0E;
const REG_BAR0: u8 = 0x10;
const REG_CAP_PTR: u8 = 0x34;

/* Device Structure */
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
	pub bus: u8,
	pub device: u8,
	pub function: u8,
	pub vendor_id: u16,
	pub device_id: u16,
}

impl PciDevice {
	/*
	 * read_u8 - Read 8-bit value from configuration space
	 */
	pub unsafe fn read_u8(&self, offset: u8) -> u8 {
		let address = 0x80000000
			| ((self.bus as u32) << 16)
			| ((self.device as u32) << 11)
			| ((self.function as u32) << 8)
			| ((offset as u32) & 0xFC);

		outl(CONFIG_ADDRESS, address);
		let val = inl(CONFIG_DATA);
		((val >> ((offset & 3) * 8)) & 0xFF) as u8
	}

	/*
	 * read_u16 - Read 16-bit value from configuration space
	 */
	pub unsafe fn read_u16(&self, offset: u8) -> u16 {
		let address = 0x80000000
			| ((self.bus as u32) << 16)
			| ((self.device as u32) << 11)
			| ((self.function as u32) << 8)
			| ((offset as u32) & 0xFC);

		outl(CONFIG_ADDRESS, address);
		let val = inl(CONFIG_DATA);
		((val >> ((offset & 2) * 8)) & 0xFFFF) as u16
	}

	/*
	 * read_u32 - Read 32-bit value from configuration space
	 */
	pub unsafe fn read_u32(&self, offset: u8) -> u32 {
		let address = 0x80000000
			| ((self.bus as u32) << 16)
			| ((self.device as u32) << 11)
			| ((self.function as u32) << 8)
			| ((offset as u32) & 0xFC);

		outl(CONFIG_ADDRESS, address);
		inl(CONFIG_DATA)
	}

	/*
	 * get_bar - Read Base Address Register
	 * @index: BAR index (0-5)
	 *
	 * Returns the physical address and size (if possible) or None.
	 * Handles 64-bit BARs automatically.
	 */
	pub unsafe fn get_bar(&self, index: u8) -> Option<(u64, u32)> {
		if index > 5 {
			return None;
		}

		let offset = REG_BAR0 + (index * 4);
		let bar_low = self.read_u32(offset);

		// Check for MMIO (bit 0 must be 0)
		if bar_low & 1 != 0 {
			return None; // IO Space not supported in this simple driver yet
		}

		// Check type (bits 1-2): 00=32bit, 10=64bit
		let bar_type = (bar_low >> 1) & 0x3;
		let mut address = (bar_low & 0xFFFFFFF0) as u64;

		if bar_type == 0x2 {
			let bar_high = self.read_u32(offset + 4);
			address |= (bar_high as u64) << 32;
		}

		// To get size, we write all 1s and read back (omitted for brevity/safety)
		Some((address, 0))
	}

	/*
	 * enable_bus_master - Enable Bus Master bit in Command Register
	 */
	pub unsafe fn enable_bus_master(&self) {
		let cmd = self.read_u16(REG_COMMAND);
		// Bit 2: Bus Master, Bit 1: Memory Space
		self.write_u16(REG_COMMAND, cmd | 0x06);
	}

	unsafe fn write_u16(&self, offset: u8, value: u16) {
		let address = 0x80000000
			| ((self.bus as u32) << 16)
			| ((self.device as u32) << 11)
			| ((self.function as u32) << 8)
			| ((offset as u32) & 0xFC);

		outl(CONFIG_ADDRESS, address);
		// Note: This naive write might overwrite adjacent bytes if not careful.
		// Proper implementation requires reading, masking, and writing back 32-bit.
		// For the Command register (aligned), simpler logic often works but verify.
		// A safer way is read-modify-write on 32-bit:
		let current = inl(CONFIG_DATA);
		let shift = (offset & 2) * 8;
		let mask = 0xFFFF << shift;
		let new_val = (current & !mask) | ((value as u32) << shift);
		outl(CONFIG_DATA, new_val);
	}

	/*
	 * find_capability - Find a specific PCI capability
	 * @cap_id: Capability ID to find (e.g., 0x09 for Vendor Specific)
	 *
	 * Returns the offset of the capability structure.
	 */
	pub unsafe fn find_capability(&self, cap_id: u8) -> Option<u8> {
		let status = self.read_u16(0x06); // Status register
		if status & 0x10 == 0 {
			return None; // Capabilities List bit not set
		}

		let mut ptr = self.read_u8(REG_CAP_PTR);
		while ptr != 0 {
			let id = self.read_u8(ptr);
			if id == cap_id {
				return Some(ptr);
			}
			ptr = self.read_u8(ptr + 1); // Next pointer
		}
		None
	}
}

pub fn enumerate_pci() -> Vec<PciDevice> {
	let mut devices = Vec::new();

	for bus in 0..=255 {
		for slot in 0..32 {
			let dummy = PciDevice {
				bus: bus as u8,
				device: slot as u8,
				function: 0,
				vendor_id: 0,
				device_id: 0,
			};

			let vendor = unsafe { dummy.read_u16(0) };
			if vendor == 0xFFFF {
				continue;
			}

			// Check header type for multi-function
			let header_type = unsafe { dummy.read_u8(REG_HEADER_TYPE) };
			let func_count = if header_type & 0x80 != 0 { 8 } else { 1 };

			for func in 0..func_count {
				let dev = PciDevice {
					bus: bus as u8,
					device: slot as u8,
					function: func as u8,
					vendor_id: 0,
					device_id: 0,
				};

				let vendor = unsafe { dev.read_u16(0) };
				if vendor != 0xFFFF {
					let device_id = unsafe { dev.read_u16(2) };
					devices.push(PciDevice {
						vendor_id: vendor,
						device_id,
						..dev
					});
				}
			}
		}
	}
	devices
}
