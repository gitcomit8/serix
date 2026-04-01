/*
 * Task Context Switching
 *
 * Low-level assembly routine for switching between task contexts.
 * Saves and restores callee-saved registers (SysV ABI) and RIP.
 *
 * Segment selectors, MSR bases, and CR3 are NOT switched here because
 * all kernel tasks share the same address space and GDT selectors.
 * CR3 switching will be added when userspace process isolation is needed.
 */

#![feature(asm_sym)]

use crate::CPUContext;
use core::arch::{asm, naked_asm};

/*
 * context_switch - Switch from one task context to another
 * @old: Pointer to CPUContext to save current state into (RDI)
 * @new: Pointer to CPUContext to restore from (RSI)
 *
 * Saves callee-saved registers (RSP, RBP, RBX, R12-R15) and RIP,
 * then restores them from the new context.  Returns when this task
 * is next scheduled.
 *
 * Stack convention: the saved RSP is the caller's RSP BEFORE the
 * `call context_switch` instruction (i.e., after popping the return
 * address). On restore, the saved RIP is pushed and `ret` is used,
 * so the caller sees the same RSP as after a normal function return.
 */
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(old: *mut CPUContext, new: *const CPUContext) {
	naked_asm!(
		/* Pop the return address — this gives us the pre-call RSP */
		"pop rax",
		"mov [rdi + 56], rax",     /* Save RIP (return address) */
		"mov [rdi + 0], rsp",      /* Save RSP (now = caller's pre-call RSP) */
		"mov [rdi + 8], rbp",
		"mov [rdi + 16], rbx",
		"mov [rdi + 24], r12",
		"mov [rdi + 32], r13",
		"mov [rdi + 40], r14",
		"mov [rdi + 48], r15",

		/* Save CR3 — CPUContext.cr3 is at offset 136 */
		"mov rax, cr3",
		"mov [rdi + 136], rax",

		/* Load new context from *new (RSI) */
		"mov rsp, [rsi + 0]",
		"mov rbp, [rsi + 8]",
		"mov rbx, [rsi + 16]",
		"mov r12, [rsi + 24]",
		"mov r13, [rsi + 32]",
		"mov r14, [rsi + 40]",
		"mov r15, [rsi + 48]",

		/* Restore CR3 — skip if unchanged to avoid TLB flush */
		"mov rax, [rsi + 136]",
		"test rax, rax",           /* cr3=0 means kernel task, keep current */
		"jz 2f",
		"mov rcx, cr3",
		"cmp rax, rcx",
		"je 2f",
		"mov cr3, rax",
		"2:",

		/* Jump to new RIP — push it so `ret` pops it and adjusts RSP */
		"push qword ptr [rsi + 56]",
		"ret",
	)
}
