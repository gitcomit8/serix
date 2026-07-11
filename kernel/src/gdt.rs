/*
 * Global Descriptor Table (GDT) & Task State Segment (TSS) Setup
 *
 * Initializes GDT with Kernel/User segments and a TSS.
 * The TSS is required for Ring 3 -> Ring 0 interrupt transitions (RSP0).
 */

use spin::{Mutex, Once};
use x86_64::VirtAddr;
use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::registers::model_specific::KernelGsBase;
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};
use x86_64::structures::tss::TaskStateSegment;

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

pub const PER_CPU_DATA_MAX_CPUS: usize = 16;

#[repr(C)]
pub struct PerCpuData {
	pub scratch: u64,
	pub kernel_stack: u64,
	pub user_stack_save: u64,
	pub cpu_id: u8,
	pub current_task_id: u64,
	pub run_queue: *const u8, // Pointer to per-CPU RunQueue
	pub tss_rsp0_save: u64,   // Save area for old TSS.RSP0
}

impl PerCpuData {
	const CPU_ID_OFFSET: u64 = 24;
	const CURRENT_TASK_ID_OFFSET: u64 = 32;
	const RUN_QUEUE_OFFSET: u64 = 40;
	const TSS_RSP0_SAVE_OFFSET: u64 = 48;
}

pub static mut PER_CPU_DATA: [PerCpuData; PER_CPU_DATA_MAX_CPUS] = {
	const INIT: PerCpuData = PerCpuData {
		scratch: 0,
		kernel_stack: 0,
		user_stack_save: 0,
		cpu_id: 0,
		current_task_id: 0,
		run_queue: core::ptr::null(),
		tss_rsp0_save: 0,
	};
	[INIT; PER_CPU_DATA_MAX_CPUS]
};

pub fn per_cpu_data(cpu_id: usize) -> &'static mut PerCpuData {
	unsafe { &mut PER_CPU_DATA[cpu_id] }
}

pub unsafe fn init_per_cpu(cpu_id: usize) {
	PER_CPU_DATA[cpu_id].cpu_id = cpu_id as u8;
	let addr = core::ptr::addr_of!(PER_CPU_DATA[cpu_id]) as u64;
	KernelGsBase::write(VirtAddr::new(addr));
}

/// Read LAPIC ID of the current CPU via MSR 0x1B.
/// Returns the APIC ID which matches BSP/AP numbering.
pub unsafe fn current_cpu_id() -> u8 {
	let mut apic_id: u64;
	core::arch::asm!("rdmsr", in("ecx") 0x1Bu32, lateout("eax") apic_id, lateout("edx") _);
	(apic_id & 0xFF) as u8
}


pub fn set_syscall_stack(stack_top: VirtAddr) {
	unsafe {
		let cpu_id = current_cpu_id() as usize;
		PER_CPU_DATA[cpu_id].kernel_stack = stack_top.as_u64();
	}
}
