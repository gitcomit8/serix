/*
 * Task Context Switching
 *
 * Low-level assembly routine for switching between task contexts.
 * Saves and restores all CPU state including registers, segments, and page tables.
 */

#![feature(asm_sym)]

use crate::CPUContext;
use core::arch::{asm, naked_asm};

/*
 * context_switch - Switch from one task context to another
 * @old: Pointer to CPUContext to save current state into
 * @new: Pointer to CPUContext to restore from
 *
 * Saves all callee-saved registers, segment state, and page table pointer,
 * then restores them from the new context. Returns when this task is resumed.
 */
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(old: *mut CPUContext, new: *const CPUContext) {
	naked_asm!(
	/* Save current context registers to *old (RDI) */
	"mov [rdi + 0], rsp",
	"mov [rdi + 8], rbp",
	"mov [rdi + 16], rbx",
	"mov [rdi + 24], r12",
	"mov [rdi + 32], r13",
	"mov [rdi + 40], r14",
	"mov [rdi + 48], r15",

	/* Save RIP (return address) from stack into old->rip */
	"mov rax, [rsp]",
	"mov [rdi + 56], rax",

	/* Save RFLAGS */
	"pushfq",
	"pop rax",
	"mov [rdi + 64], rax",

	/* Save segment selectors */
	"mov ax, cs",
	"mov [rdi + 72], rax",
	"mov ax, ss",
	"mov [rdi + 80], rax",
	"mov ax, fs",
	"mov [rdi + 88], rax",
	"mov ax, gs",
	"mov [rdi + 96], rax",
	"mov ax, ds",
	"mov [rdi + 104], rax",
	"mov ax, es",
	"mov [rdi + 112], rax",

	// Save FS_BASE
	"mov ecx, 0xC0000100",
	"rdmsr",
	"shl rdx, 32",
	"or rax, rdx",
	"mov [rdi + 120], rax",

	// Save GS_BASE
	"mov ecx, 0xC0000101",
	"rdmsr",
	"shl rdx, 32",
	"or rax, rdx",
	"mov [rdi + 128], rax",

	// Save CR3
	"mov rax, cr3",
	"mov [rdi + 136], rax",

	// Load registers from *new
	"mov rsp, [rsi + 0]",
	"mov rbp, [rsi + 8]",
	"mov rbx, [rsi + 16]",
	"mov r12, [rsi + 24]",
	"mov r13, [rsi + 32]",
	"mov r14, [rsi + 40]",
	"mov r15, [rsi + 48]",

	// Restore segment registers (ds, es, fs, gs)
	"mov ax, [rsi + 104]",
	"mov ds, ax",
	"mov ax, [rsi + 112]",
	"mov es, ax",
	"mov ax, [rsi + 88]",
	"mov fs, ax",
	"mov ax, [rsi + 96]",
	"mov gs, ax",

	// Restore FS_BASE
	"mov ecx, 0xC0000100",
	"mov rax, [rsi + 120]",
	"mov rdx, rax",
	"shr rdx, 32",
	"mov eax, eax",
	"wrmsr",

	// Restore GS_BASE
	"mov ecx, 0xC0000101",
	"mov rax, [rsi + 128]",
	"mov rdx, rax",
	"shr rdx, 32",
	"mov eax, eax",
	"wrmsr",

	// Restore CR3
	"mov rax, [rsi + 136]",
	"mov cr3, rax",

	// Restore RFLAGS
	"mov rax, [rsi + 64]",
	"push rax",
	"popfq",

	// Jump to new RIP by pushing it and returning
	"push qword ptr [rsi + 56]",
	"ret",
	)
}
