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

/*
 * PCI Configuration Register Offsets
 */
const REG_VENDOR_ID: u8 = 0x00;
const REG_COMMAND: u8 = 0x04;
const REG_HEADER_TYPE: u8 = 0x0E;
const REG_BAR0: u8 = 0x10;
const REG_CAP_PTR: u8 = 0x34;
const REG_CLASS_REVISION: u8 = 0x08;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceClass {
	Nvme,
	Xhci,
	Other {
		class: u8,
		subclass: u8,
		prog_if: u8,
	},
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarKind {
	Memory32,
	Memory64,
	Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BarInfo {
	pub index: u8,
	pub kind: BarKind,
	pub base: u64,
	pub prefetchable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MsixInfo {
	pub table_size: u16,
	pub table_bir: u8,
	pub table_offset: u32,
	pub pba_bir: u8,
	pub pba_offset: u32,
}

/*
 * struct PciDevice - Represents a PCI device
 * @bus: Bus number
 * @device: Device number
 * @function: Function number
 * @vendor_id: Vendor ID
 * @device_id: Device ID
 */
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
	pub bus: u8,
	pub device: u8,
	pub function: u8,
	pub vendor_id: u16,
	pub device_id: u16,
	pub class_code: u8,
	pub subclass: u8,
	pub prog_if: u8,
}

impl PciDevice {
	pub fn class(&self) -> DeviceClass {
		match (self.class_code, self.subclass, self.prog_if) {
			(0x01, 0x08, 0x02) => DeviceClass::Nvme,
			(0x0c, 0x03, 0x30) => DeviceClass::Xhci,
			(class, subclass, prog_if) => DeviceClass::Other {
				class,
				subclass,
				prog_if,
			},
		}
	}

	/// Decode the assigned BAR without writing all ones to config space.
	/// BAR length must come from trusted firmware/resource assignment until a
	/// quiesced probing mechanism is implemented.
	pub unsafe fn bar_info(&self, index: u8) -> Option<BarInfo> {
		if index > 5 {
			return None;
		}
		let offset = REG_BAR0 + index * 4;
		let low = self.read_u32(offset);
		if low & 1 != 0 {
			return Some(BarInfo {
				index,
				kind: BarKind::Io,
				base: (low & !3) as u64,
				prefetchable: false,
			});
		}
		let ty = (low >> 1) & 3;
		let base = match ty {
			0 => (low & !0xF) as u64,
			2 if index < 5 => (self.read_u32(offset + 4) as u64) << 32 | (low & !0xF) as u64,
			_ => return None,
		};
		Some(BarInfo {
			index,
			kind: if ty == 2 {
				BarKind::Memory64
			} else {
				BarKind::Memory32
			},
			base,
			prefetchable: low & 8 != 0,
		})
	}

	/// Read MSI-X layout metadata. This does not enable MSI-X or expose its table.
	pub unsafe fn msix_info(&self) -> Option<MsixInfo> {
		let ptr = self.find_capability(0x11)?;
		let control = self.read_u16(ptr + 2);
		let table = self.read_u32(ptr + 4);
		let pba = self.read_u32(ptr + 8);
		Some(MsixInfo {
			table_size: (control & 0x07ff) + 1,
			table_bir: (table & 7) as u8,
			table_offset: table & !7,
			pba_bir: (pba & 7) as u8,
			pba_offset: pba & !7,
		})
	}
	/*
	 * read_u8 - Read 8-bit value from configuration space
	 * @offset: Register offset to read from
	 *
	 * Return: 8-bit value at the specified offset
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
	 * @offset: Register offset to read from
	 *
	 * Return: 16-bit value at the specified offset
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
	 * @offset: Register offset to read from
	 *
	 * Return: 32-bit value at the specified offset
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
	 *
	 * Return: Option containing (physical_address, size) tuple
	 */
	pub unsafe fn get_bar(&self, index: u8) -> Option<(u64, u32)> {
		let bar = self.bar_info(index)?;
		if bar.kind == BarKind::Io {
			None
		} else {
			Some((bar.base, 0))
		}
	}

	/*
	 * enable_bus_master - Enable Bus Master bit in Command Register
	 *
	 * Enables DMA and memory space access for the device.
	 */
	pub unsafe fn enable_bus_master(&self) {
		let cmd = self.read_u16(REG_COMMAND);
		// Bit 2: Bus Master, Bit 1: Memory Space
		self.write_u16(REG_COMMAND, cmd | 0x06);
	}

	/*
	 * write_u16 - Write 16-bit value to configuration space
	 * @offset: Register offset to write to
	 * @value: Value to write
	 *
	 * Uses read-modify-write to preserve adjacent bytes.
	 */
	unsafe fn write_u16(&self, offset: u8, value: u16) {
		let address = 0x80000000
			| ((self.bus as u32) << 16)
			| ((self.device as u32) << 11)
			| ((self.function as u32) << 8)
			| ((offset as u32) & 0xFC);

		outl(CONFIG_ADDRESS, address);
		/*
		 * Note: This naive write might overwrite adjacent bytes if not careful.
		 * Proper implementation requires reading, masking, and writing back 32-bit.
		 * For the Command register (aligned), simpler logic often works but verify.
		 * A safer way is read-modify-write on 32-bit:
		 */
		let current = inl(CONFIG_DATA);
		let shift = (offset & 2) * 8;
		let mask = 0xFFFF << shift;
		let new_val = (current & !mask) | ((value as u32) << shift);
		outl(CONFIG_DATA, new_val);
	}

	/*
	 * interrupt_line - Read PCI interrupt line register
	 *
	 * Return: IRQ number assigned by firmware (config offset 0x3C)
	 */
	pub unsafe fn interrupt_line(&self) -> u8 {
		self.read_u8(0x3C)
	}

	/*
	 * interrupt_pin - Read PCI interrupt pin register
	 *
	 * Return: Interrupt pin (1=INTA, 2=INTB, etc., 0=none)
	 */
	pub unsafe fn interrupt_pin(&self) -> u8 {
		self.read_u8(0x3D)
	}

	/*
	 * find_capability - Find a specific PCI capability
	 * @cap_id: Capability ID to find (e.g., 0x09 for Vendor Specific)
	 *
	 * Walks the capability list looking for the specified capability ID.
	 *
	 * Return: Option containing capability offset, or None if not found
	 */
	pub unsafe fn find_capability(&self, cap_id: u8) -> Option<u8> {
		let status = self.read_u16(0x06); // Status register
		if status & 0x10 == 0 {
			return None; // Capabilities List bit not set
		}

		let mut ptr = self.read_u8(REG_CAP_PTR) & !3;
		let mut visited = [false; 64];
		while ptr != 0 {
			if !(0x40..=0xfc).contains(&ptr) {
				return None;
			}
			let slot = ((ptr - 0x40) / 4) as usize;
			if visited[slot] {
				return None;
			}
			visited[slot] = true;
			let id = self.read_u8(ptr);
			if id == cap_id {
				return Some(ptr);
			}
			ptr = self.read_u8(ptr + 1) & !3; // Next pointer, DWORD aligned
		}
		None
	}
}

/*
 * enumerate_pci - Scan the PCI bus for all devices
 *
 * Enumerates all PCI devices by probing all possible bus/device/function
 * combinations and checking vendor IDs.
 *
 * Return: Vector of all discovered PCI devices
 */
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
				class_code: 0,
				subclass: 0,
				prog_if: 0,
			};

			let vendor = unsafe { dummy.read_u16(0) };
			if vendor == 0xFFFF {
				continue;
			}

			// Check header type for multi-function device
			let header_type = unsafe { dummy.read_u8(REG_HEADER_TYPE) };
			let func_count = if header_type & 0x80 != 0 { 8 } else { 1 };

			for func in 0..func_count {
				let dev = PciDevice {
					bus: bus as u8,
					device: slot as u8,
					function: func as u8,
					vendor_id: 0,
					device_id: 0,
					class_code: 0,
					subclass: 0,
					prog_if: 0,
				};

				let vendor = unsafe { dev.read_u16(0) };
				if vendor != 0xFFFF {
					let device_id = unsafe { dev.read_u16(2) };
					let class_revision = unsafe { dev.read_u32(REG_CLASS_REVISION) };
					devices.push(PciDevice {
						vendor_id: vendor,
						device_id,
						class_code: (class_revision >> 24) as u8,
						subclass: (class_revision >> 16) as u8,
						prog_if: (class_revision >> 8) as u8,
						..dev
					});
				}
			}
		}
	}
	devices
}
