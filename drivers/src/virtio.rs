use crate::pci::PciDevice;

pub struct VirtioBlock;

impl VirtioBlock {
	pub fn init(dev: PciDevice) -> Option<Self> {
		if dev.vendor_id == 0x1AF4 && dev.device_id == 0x1001 {
			return Some(Self);
		}
		None
	}
}
