; boot.s - Minimal multiboot2-compliant boot entry for Serix kernel
; NASM syntax, 64-bit flat binary

section .text
global _start
extern rust_main

_start:
    ; Set up stack pointer (adjust as needed)
    mov rsp, stack_top

    ; Move multiboot magic and info parameters into registers as needed
    ; GRUB passes magic in rdi and multiboot info pointer in rsi, or in eax and ebx for 32-bit
    ; Here, assume rdi = magic, rsi = info pointer (x86_64 System V calling convention)
    
    ; Push arguments on the stack for rust_main
    push rsi       ; multiboot info pointer
    push rdi       ; multiboot magic number

    call rust_main

    ; If rust_main returns, halt the system
.halt:
    cli
    hlt
    jmp .halt

section .bss
    align 16
stack_bottom:
    resb 16384         ; 16 KiB stack
stack_top:
