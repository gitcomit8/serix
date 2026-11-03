//! Kernel-owned, bounded inter-processor interrupt mailboxes.
//!
//! Callers provide logical CPU IDs; the module translates them to firmware
//! APIC IDs. Vector assignments are reserved here and must not be used for MSI.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use x86_64::structures::idt::InterruptStackFrame;

pub const TLB_SHOOTDOWN_VECTOR: u8 = 0xF0;
pub const SCHED_KICK_VECTOR: u8 = 0xF1;
pub const PANIC_STOP_VECTOR: u8 = 0xF2;
const MAX_IPI_CPUS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ShootdownError {
	BadCpu,
	CpuOffline,
	Busy,
	DeliveryTimeout,
	AckTimeout,
}

struct Mailbox {
	apic_id: AtomicU32,
	configured: AtomicBool,
	online: AtomicBool,
	busy: AtomicBool,
	address: AtomicU64,
	full_flush: AtomicBool,
	sequence: AtomicU64,
	acknowledged: AtomicU64,
	sched_pending: AtomicBool,
}

impl Mailbox {
	const fn new() -> Self {
		Self {
			apic_id: AtomicU32::new(0),
			configured: AtomicBool::new(false),
			online: AtomicBool::new(false),
			busy: AtomicBool::new(false),
			address: AtomicU64::new(0),
			full_flush: AtomicBool::new(false),
			sequence: AtomicU64::new(0),
			acknowledged: AtomicU64::new(0),
			sched_pending: AtomicBool::new(false),
		}
	}
}

static MAILBOXES: [Mailbox; MAX_IPI_CPUS] = [const { Mailbox::new() }; MAX_IPI_CPUS];
static NEXT_SEQUENCE: AtomicU64 = AtomicU64::new(1);

pub fn register_handlers() {
	unsafe {
		idt::register_interrupt_handler(TLB_SHOOTDOWN_VECTOR, tlb_shootdown_handler);
		idt::register_interrupt_handler(SCHED_KICK_VECTOR, sched_kick_handler);
		idt::register_interrupt_handler(PANIC_STOP_VECTOR, panic_stop_handler);
	}
}

/// Register the logical-to-physical APIC mapping before that CPU can receive work.
pub fn configure_cpu(logical_id: usize, apic_id: u32) -> Result<(), ShootdownError> {
	let mailbox = MAILBOXES.get(logical_id).ok_or(ShootdownError::BadCpu)?;
	mailbox.apic_id.store(apic_id, Ordering::Relaxed);
	mailbox.configured.store(true, Ordering::Release);
	Ok(())
}

/// Publish that a CPU has completed its local interrupt and scheduler setup.
pub fn set_cpu_online(logical_id: usize) -> Result<(), ShootdownError> {
	let mailbox = MAILBOXES.get(logical_id).ok_or(ShootdownError::BadCpu)?;
	if !mailbox.configured.load(Ordering::Acquire) {
		return Err(ShootdownError::BadCpu);
	}
	mailbox.online.store(true, Ordering::Release);
	Ok(())
}

/// Request a local INVLPG on every listed remote CPU and wait for each ack.
/// The caller must flush its own CPU before calling and must not pass itself.
/// The caller must keep any unmapped frames alive until this returns Ok.
/// An IPI delivery/ack timeout quarantines that mailbox as busy; callers must
/// treat the shootdown as failed and must not reclaim the affected frames.
pub fn shootdown(
	logical_cpus: &[usize],
	address: Option<u64>,
	timeout_spins: usize,
) -> Result<u64, ShootdownError> {
	let sequence = NEXT_SEQUENCE.fetch_add(1, Ordering::Relaxed);
	for &logical_id in logical_cpus {
		let mailbox = MAILBOXES.get(logical_id).ok_or(ShootdownError::BadCpu)?;
		if !mailbox.online.load(Ordering::Acquire) {
			return Err(ShootdownError::CpuOffline);
		}
		if mailbox
			.busy
			.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
			.is_err()
		{
			return Err(ShootdownError::Busy);
		}
		mailbox
			.address
			.store(address.unwrap_or(0), Ordering::Relaxed);
		mailbox
			.full_flush
			.store(address.is_none(), Ordering::Relaxed);
		mailbox.sequence.store(sequence, Ordering::Release);
		if !super::send_ipi(
			mailbox.apic_id.load(Ordering::Relaxed),
			TLB_SHOOTDOWN_VECTOR as u32,
		) {
			return Err(ShootdownError::DeliveryTimeout);
		}
		let mut spins = timeout_spins;
		while mailbox.acknowledged.load(Ordering::Acquire) != sequence {
			if spins == 0 {
				return Err(ShootdownError::AckTimeout);
			}
			spins -= 1;
			core::hint::spin_loop();
		}
		mailbox.busy.store(false, Ordering::Release);
	}
	Ok(sequence)
}

/// Coalesced scheduler hint. Scheduling policy remains owned by the target CPU.
pub fn scheduler_kick(logical_id: usize) -> Result<(), ShootdownError> {
	let mailbox = MAILBOXES.get(logical_id).ok_or(ShootdownError::BadCpu)?;
	if !mailbox.online.load(Ordering::Acquire) {
		return Err(ShootdownError::CpuOffline);
	}
	mailbox.sched_pending.store(true, Ordering::Release);
	if !super::send_ipi(
		mailbox.apic_id.load(Ordering::Relaxed),
		SCHED_KICK_VECTOR as u32,
	) {
		return Err(ShootdownError::DeliveryTimeout);
	}
	Ok(())
}

pub fn take_scheduler_kick(logical_id: usize) -> bool {
	MAILBOXES
		.get(logical_id)
		.is_some_and(|m| m.sched_pending.swap(false, Ordering::AcqRel))
}

fn find_current_mailbox() -> Option<&'static Mailbox> {
	let id = unsafe { super::smp::read_apic_id() };
	MAILBOXES
		.iter()
		.find(|m| m.configured.load(Ordering::Acquire) && m.apic_id.load(Ordering::Relaxed) == id)
}

extern "x86-interrupt" fn tlb_shootdown_handler(_frame: InterruptStackFrame) {
	if let Some(mailbox) = find_current_mailbox() {
		let seq = mailbox.sequence.load(Ordering::Acquire);
		let address = mailbox.address.load(Ordering::Relaxed);
		if mailbox.full_flush.load(Ordering::Relaxed) {
			unsafe {
				let cr3: u64;
				core::arch::asm!("mov {}, cr3", out(reg) cr3, options(nomem, nostack, preserves_flags));
				core::arch::asm!("mov cr3, {}", in(reg) cr3, options(nomem, nostack, preserves_flags));
			}
		} else {
			unsafe {
				core::arch::asm!("invlpg [{}]", in(reg) address, options(nostack, preserves_flags));
			}
		}
		mailbox.acknowledged.store(seq, Ordering::Release);
	}
	unsafe {
		super::send_eoi();
	}
}

extern "x86-interrupt" fn sched_kick_handler(_frame: InterruptStackFrame) {
	unsafe {
		super::send_eoi();
	}
}

extern "x86-interrupt" fn panic_stop_handler(_frame: InterruptStackFrame) {
	unsafe {
		super::send_eoi();
	}
	x86_64::instructions::interrupts::disable();
	loop {
		x86_64::instructions::hlt();
	}
}
