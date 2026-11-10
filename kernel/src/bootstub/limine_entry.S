/*
 * bootstub/limine_entry.S - Limine kernel entry point
 *
 * Called by Limine bootloader after the bootstrap code has parsed
 * Limine requests and populated the response structures.
 * Jumps to limine_boot() in Rust.
 */
.extern limine_boot
.globl _limine_entry
.type _limine_entry, @function

_limine_entry:
    jmp limine_boot
