/*
 * Task Context Switching
 *
 * Low-level assembly routine for switching between task contexts.
 * Saves and restores callee-saved registers (SysV ABI), RIP, and CR3.
 * Updates PER_CPU_DATA.current_task_id to the new task.
 *
 * TSS.RSP0 is updated via the SWITCH_HOOK (registered by the kernel
 * in gdt.rs) which fires before each context switch. The hook calls
 * `set_kernel_stack()` which writes to TSS.privilege_stack_table[0].
 * For kernel→kernel switches the hook fires inside `schedule()`.
 * For kernel→userspace transitions, the kernel entry path sets TSS.RSP0.
 *
 * Segment selectors and MSR bases are NOT switched here because
 * all kernel tasks share the same address space and GDT selectors.
 */

#![feature(asm_sym)]

use crate::CPUContext;
use core::arch::naked_asm;

/*
 * context_switch - Switch from one task context to another
 * @old: Pointer to CPUContext to save current state into (RDI)
 * @new: Pointer to CPUContext to restore from (RSI)
 *
 * Saves callee-saved registers (RSP, RBP, RBX, R12-R15) and RIP,
 * then restores them from the new context. Records the new task's
 * ID in PER_CPU_DATA (GS_BASE points to the current CPU's data area).
 *
 * Register-mediated IPC: if the new context has ipc_msg_valid set and
 * ipc_msg_ptr non-zero, the message pointer is loaded into rbx, size
 * into r12, and opcode into r13 BEFORE normal register restoration.
 * This places the IPC data in SysV calling-convention argument registers
 * (rdi=rbx, rsi=r12, rdx=r13) so the receiver can access it directly.
 *
 * Stack convention: the saved RSP is the caller's RSP BEFORE the
 * `call context_switch` instruction (i.e., after popping the return
 * address). On restore, the saved RIP is pushed and `ret` is used,
 * so the caller sees the same RSP as after a normal function return.
 *
 * CPUContext field offsets (#[repr(C)], 8-byte fields):
 *   rsp=0, rbp=8, rbx=16, r12=24, r13=32, r14=40, r15=48
 *   rip=56, rflags=64, cs=72, ss=80, fs=88, gs=96, ds=104, es=112
 *   fs_base=120, gs_base=128, cr3=136, ipc_msg_ptr=144, ipc_msg_valid=152
 */
#[unsafe(naked)]
pub unsafe extern "C" fn context_switch(old: *mut CPUContext, new: *const CPUContext) {
	naked_asm!(
		/* ---- Save current context (old = RDI) ---- */
		/* Pop the return address — this gives us the pre-call RSP */
		"pop rax",
		"mov [rdi + 56], rax", /* Save RIP (return address) */
		"mov [rdi + 0], rsp",  /* Save RSP (now = caller's pre-call RSP) */
		"mov [rdi + 8], rbp",
		"mov [rdi + 16], rbx",
		"mov [rdi + 24], r12",
		"mov [rdi + 32], r13",
		"mov [rdi + 40], r14",
		"mov [rdi + 48], r15",
		/* Save CR3 — CPUContext.cr3 is at offset 136 */
		"mov rax, cr3",
		"mov [rdi + 136], rax",

		/* ---- Load new context ---- */
		"mov rsp, [rsi + 0]",
		"mov rbp, [rsi + 8]",
		"mov rbx, [rsi + 16]",
		"mov r12, [rsi + 24]",
		"mov r13, [rsi + 32]",
		"mov r14, [rsi + 40]",
		"mov r15, [rsi + 48]",

		/* ---- Register-mediated IPC: load after normal restore ---- */
		/* Overwrite rbx/r12/r13 with IPC data if flag is set.
		 * These are the SysV first/second/third integer arg registers,
		 * so the receiver sees IPC data as function arguments.
		 * IPC fields stay in CPUContext for consume_direct_message to read. */
		"mov cl, byte ptr [rsi + 152]", /* ipc_msg_valid */
		"test cl, cl",
		"jz 2f",
		"mov rbx, [rsi + 144]", /* ipc_msg_ptr → rbx (arg1 = msg ptr) */
		"mov r12, [rsi + 156]", /* padding/size → r12 (arg2 = size) */
		"mov r13, [rsi + 160]", /* padding → r13 (arg3 = opcode) */
		"2:",

		/* ---- Restore CR3 — skip if unchanged to avoid TLB flush ---- */
		"mov rax, [rsi + 136]",
		"test rax, rax", /* cr3=0 means kernel task, keep current */
		"jz 2f",
		"mov rcx, cr3",
		"cmp rax, rcx",
		"je 2f",
		"mov cr3, rax",
		"2:",

		/* ---- Update PER_CPU_DATA.current_task_id ---- */
		/* current_task_id is already set by pick_next_task() before
		 * this call. We read it here to confirm, but the value was
		 * established by the scheduler. */

		/* Jump to new RIP — push it so `ret` pops it and adjusts RSP */
		"push qword ptr [rsi + 56]",
		"ret",
	)
}
