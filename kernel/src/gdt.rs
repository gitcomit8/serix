use spin::Once;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};

static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

pub struct Selectors {
	pub kernel_code: SegmentSelector,
	pub kernel_data: SegmentSelector,
	pub user_data: SegmentSelector,
	pub user_code: SegmentSelector,
}

pub fn init() {
	let (gdt, selectors) = GDT.call_once(|| {
		let mut gdt = GlobalDescriptorTable::new();

		// Kernel Segments
		let kernel_code = gdt.append(Descriptor::kernel_code_segment()); // Index 1
		let kernel_data = gdt.append(Descriptor::kernel_data_segment()); // Index 2

		// User Segments Layout for SYSRET:
		// Index 3: User Base (Unused in 64-bit, but required for offset)
		// Index 4: User Data (SS)
		// Index 5: User Code (CS)

		// We just duplicate user_data for the base placeholder
		gdt.append(Descriptor::user_data_segment()); // Index 3 (Base)

		let user_data = gdt.append(Descriptor::user_data_segment()); // Index 4
		let user_code = gdt.append(Descriptor::user_code_segment()); // Index 5

		(
			gdt,
			Selectors {
				kernel_code,
				kernel_data,
				user_data,
				user_code,
			},
		)
	});

	gdt.load();

	unsafe {
		CS::set_reg(selectors.kernel_code);
		SS::set_reg(selectors.kernel_data);
		DS::set_reg(selectors.kernel_data);
		ES::set_reg(selectors.kernel_data);
		FS::set_reg(selectors.kernel_data);
		GS::set_reg(selectors.kernel_data);
	}
}
