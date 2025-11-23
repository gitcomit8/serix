/*
 * Global Descriptor Table (GDT) & Task State Segment (TSS) Setup
 *
 * Initializes GDT with Kernel/User segments and a TSS.
 * The TSS is required for Ring 3 -> Ring 0 interrupt transitions (RSP0).
 */

use spin::{Mutex, Once};
use x86_64::instructions::segmentation::{Segment, CS, DS, ES, FS, GS, SS};
use x86_64::instructions::tables::load_tss;
use x86_64::registers::model_specific::KernelGsBase;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;
use x86_64::VirtAddr;

/* Global GDT and TSS instances */
static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();
static TSS: Once<Mutex<TaskStateSegment>> = Once::new();

pub struct Selectors {
	pub kernel_code: SegmentSelector,
	pub kernel_data: SegmentSelector,
	pub user_data: SegmentSelector,
	pub user_code: SegmentSelector,
	pub tss_selector: SegmentSelector,
}

/*
 * init - Initialize GDT and TSS
 */
pub fn init() {
	// 1. Initialize TSS
	let tss = TSS.call_once(|| {
		let mut tss = TaskStateSegment::new();
		// Set up Interrupt Stack Table (IST) for Double Faults
		// (Allocating a small static stack for safety)
		tss.interrupt_stack_table[0] = {
			const STACK_SIZE: usize = 4096 * 5;
			static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

			// FIX: Use addr_of! to get pointer without creating a reference
			let stack_start = VirtAddr::from_ptr(unsafe { core::ptr::addr_of!(STACK) });
			let stack_end = stack_start + STACK_SIZE as u64;
			stack_end
		};
		Mutex::new(tss)
	});

	// 2. Initialize GDT
	let (gdt, selectors) = GDT.call_once(|| {
		let mut gdt = GlobalDescriptorTable::new();

		// Index 1: Kernel Code
		let kernel_code = gdt.append(Descriptor::kernel_code_segment());
		// Index 2: Kernel Data
		let kernel_data = gdt.append(Descriptor::kernel_data_segment());

		// Index 3: User Data Base (Unused in 64-bit, placeholder for SYSRET)
		gdt.append(Descriptor::user_data_segment());

		// Index 4: User Data (SS)
		let user_data = gdt.append(Descriptor::user_data_segment());
		// Index 5: User Code (CS)
		let user_code = gdt.append(Descriptor::user_code_segment());

		// Index 6-7: TSS (Takes 2 slots)
		// We need a static reference to the TSS. Since TSS is in a static Once,
		// the address is stable. We unsafe-cast the pointer to bypass the Mutex lock lifetime.
		let tss_ref = unsafe {
			let guard = tss.lock();
			let ptr = &*guard as *const TaskStateSegment;
			drop(guard);
			&*ptr
		};

		let tss_selector = gdt.append(Descriptor::tss_segment(tss_ref));

		(
			gdt,
			Selectors {
				kernel_code,
				kernel_data,
				user_data,
				user_code,
				tss_selector,
			},
		)
	});

	// 3. Load GDT
	gdt.load();

	// 4. Reload Segment Registers
	unsafe {
		CS::set_reg(selectors.kernel_code);
		SS::set_reg(selectors.kernel_data);

		// Load TSS (Critical for Ring 3 interrupts)
		load_tss(selectors.tss_selector);

		// Reset data segments
		DS::set_reg(selectors.kernel_data);
		ES::set_reg(selectors.kernel_data);
		FS::set_reg(selectors.kernel_data);
		GS::set_reg(selectors.kernel_data);
	}
}

pub fn descriptors() -> &'static Selectors {
	&GDT.get().expect("GDT not initialized").1
}

/*
 * set_kernel_stack - Update the RSP0 in TSS
 *
 * Called during context switch to ensure the CPU knows where to
 * save state when an interrupt occurs in User Mode.
 */
pub fn set_kernel_stack(stack_top: VirtAddr) {
	if let Some(tss_mutex) = TSS.get() {
		let mut tss = tss_mutex.lock();
		tss.privilege_stack_table[0] = stack_top;
	}
}

#[repr(C)]
pub struct PerCpuData {
	pub scratch: u64,         // 0x00
	pub kernel_stack: u64,    // 0x08
	pub user_stack_save: u64, // 0x10
}

static mut PER_CPU_DATA: PerCpuData = PerCpuData {
	scratch: 0,
	kernel_stack: 0,
	user_stack_save: 0,
};

pub unsafe fn init_per_cpu() {
	let addr = core::ptr::addr_of!(PER_CPU_DATA) as u64;
	KernelGsBase::write(VirtAddr::new(addr));
}

pub fn set_syscall_stack(stack_top: VirtAddr) {
	unsafe {
		PER_CPU_DATA.kernel_stack = stack_top.as_u64();
	}
}
