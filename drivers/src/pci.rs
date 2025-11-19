extern crate alloc;
use alloc::vec::Vec;
use hal::io::{inl, outl};

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
	pub bus: u8,
	pub device: u8,
	pub function: u8,
	pub vendor_id: u16,
	pub device_id: u16,
}

unsafe fn pci_read32(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
	let address = 0x80000000
		| ((bus as u32) << 16)
		| ((slot as u32) << 11)
		| ((func as u32) << 8)
		| ((offset as u32) & 0xFC);

	outl(CONFIG_ADDRESS, address);
	inl(CONFIG_DATA)
}

unsafe fn pci_read16(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
	let val = pci_read32(bus, slot, func, offset);
	((val >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

pub fn enumerate_pci() -> Vec<PciDevice> {
	let mut devices = Vec::new();

	for bus in 0..255 {
		for slot in 0..32 {
			let vendor = unsafe { pci_read16(bus, slot, 0, 0) };
			if vendor != 0xFFFF {
				let device_id = unsafe { pci_read16(bus, slot, 0, 2) };
				devices.push(PciDevice {
					bus,
					device: slot,
					function: 0,
					vendor_id: vendor,
					device_id,
				});
			}
		}
	}
	devices
}
