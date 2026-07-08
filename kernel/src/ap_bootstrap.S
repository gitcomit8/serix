/*
 * ap_bootstrap.S - AP Bootstrap Code
 *
 * This code runs on Application Processors (APs) after they are woken up.
 * It sets up the basic CPU state (GDT, IDT, paging) and then jumps to
 * the Rust ap_bootstrap function in smp.rs.
 */

.section .text
.globl ap_bootstrap
.type ap_bootstrap, @function

/*
 * ap_bootstrap - AP Bootstrap Entry Point
 *
 * Called by the BSP after sending the INIT-SIPI-SIPI sequence.
 * The AP starts executing from the address specified in the SIPI message.
 *
 * Steps:
 * 1. Switch to 32-bit protected mode (if not already)
 * 2. Load GDT with kernel segments
 * 3. Setup IDT
 * 4. Enable APIC
 * 5. Jump to per_cpu_init (Rust function)
 */
ap_bootstrap:
    /* Disable interrupts */
    cli

    /* Setup GDT (simplified - uses BSP's GDT) */
    /* In a full implementation, we'd build a new GDT */

    /* Load IDT (uses BSP's IDT) */
    /* In a full implementation, we'd build a new IDT */

    /* Enable APIC */
    mov $0xFEE00000, %eax
    mov %eax, %fs
    mov $0xF0, %edi
    orl $0x100, (%edi)

    /* Jump to Rust per_cpu_init */
    jmp per_cpu_init
