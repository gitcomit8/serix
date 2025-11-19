use spin::Once;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};

static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

pub struct Selectors {
	pub kernel_code: SegmentSelector,
	pub kernel_data: SegmentSelector,
	pub user_code: SegmentSelector,
	pub user_data: SegmentSelector,
}

pub fn init() {
	let (gdt, selectors) = GDT.call_once(|| {
		let mut gdt = GlobalDescriptorTable::new();

		let kernel_code = gdt.append(Descriptor::kernel_code_segment());
		let kernel_data = gdt.append(Descriptor::kernel_data_segment());
		let user_code = gdt.append(Descriptor::user_code_segment());
		let user_data = gdt.append(Descriptor::user_data_segment());

		(
			gdt,
			Selectors {
				kernel_code,
				kernel_data,
				user_code,
				user_data,
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
