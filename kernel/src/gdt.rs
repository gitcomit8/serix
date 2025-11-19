/*
 * Global Descriptor Table (GDT) Setup
 *
 * Initializes the GDT with kernel and user segments for x86_64 long mode.
 * The layout is designed to be compatible with SYSRET instruction.
 */

use spin::Once;
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};

/* Global GDT instance with segment selectors */
static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

/*
 * struct Selectors - Segment selector indices
 * @kernel_code: Kernel code segment selector (Ring 0)
 * @kernel_data: Kernel data segment selector (Ring 0)
 * @user_data: User data segment selector (Ring 3)
 * @user_code: User code segment selector (Ring 3)
 */
pub struct Selectors {
	pub kernel_code: SegmentSelector,
	pub kernel_data: SegmentSelector,
	pub user_data: SegmentSelector,
	pub user_code: SegmentSelector,
}

/*
 * init - Initialize and load the Global Descriptor Table
 *
 * Creates a GDT with proper segment layout for kernel and user mode.
 * The user segments are ordered specifically for SYSRET compatibility.
 */
pub fn init() {
	let (gdt, selectors) = GDT.call_once(|| {
		let mut gdt = GlobalDescriptorTable::new();

		/*
		 * Kernel Segments (Ring 0)
		 * Index 1: Kernel code segment
		 * Index 2: Kernel data segment
		 */
		let kernel_code = gdt.append(Descriptor::kernel_code_segment());
		let kernel_data = gdt.append(Descriptor::kernel_data_segment());

		/*
		 * User Segments Layout for SYSRET compatibility (Ring 3)
		 * Index 3: User Base (Unused in 64-bit, but required for offset calculation)
		 * Index 4: User Data (SS)
		 * Index 5: User Code (CS)
		 *
		 * SYSRET expects: SS = STAR + 8, CS = STAR + 16
		 */
		gdt.append(Descriptor::user_data_segment());

		let user_data = gdt.append(Descriptor::user_data_segment());
		let user_code = gdt.append(Descriptor::user_code_segment());

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

	/* Load GDT into CPU */
	gdt.load();

	/* Update segment registers to use kernel segments */
	unsafe {
		CS::set_reg(selectors.kernel_code);
		SS::set_reg(selectors.kernel_data);
		DS::set_reg(selectors.kernel_data);
		ES::set_reg(selectors.kernel_data);
		FS::set_reg(selectors.kernel_data);
		GS::set_reg(selectors.kernel_data);
	}
}
