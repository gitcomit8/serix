/*
 * bootstub/grub2_entry.S - GRUB2 Multiboot2 kernel entry point
 *
 * Includes a multiboot2 header so GRUB2 can load this kernel.
 * The entry point parses rsi (multiboot2 info pointer) and calls
 * grub2_boot() in Rust.
 */

/* Multiboot2 header */
.section .mbheader, "a"
.align 8
multiboot2_header:
    /* Magic */
    .long 0xe85250d6
    /* Architecture 0 (x86) */
    .long 0
    /* Header length */
    .long mbheader_end - multiboot2_header
    /* Checksum */
    .long 0x100000000 - (0xe85250d6 + 0 + (mbheader_end - multiboot2_header))

/* Tags */
.align 8
    /* Code size tag */
    .short 1      /* type */
    .short 0      /* flags */
    .long mbheader_code_size - multiboot2_header  /* tag */
    .long 0       /* tag size */
mbheader_code_size:

.align 8
    /* Entry address tag */
    .short 2      /* type */
    .short 0      /* flags */
    .long _grub2_entry - multiboot2_header  /* address */
    .long 8       /* tag size */

.align 8
    /* Header address tag */
    .short 3      /* type */
    .short 0      /* flags */
    .long multiboot2_header - multiboot2_header  /* address */
    .long 12      /* tag size */

.align 8
    /* Load address tag */
    .short 4      /* type */
    .short 0      /* flags */
    .long 0xffffffff80000000  /* load address */
    .long 8       /* tag size */

.align 8
    /* End tag */
    .short 0      /* type */
    .short 0      /* flags */
    .long 0       /* tag size */
mbheader_end:

/* Entry point */
.extern grub2_boot
.globl _grub2_entry
.type _grub2_entry, @function

_grub2_entry:
    /* RAX = multiboot magic (ignored) */
    /* RSI = pointer to multiboot2 info structure */
    mov %rsi, %rdi    /* pass info pointer as first arg */
    jmp grub2_boot
