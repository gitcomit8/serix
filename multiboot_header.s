; multiboot_header.s - minimal multiboot2 header in NASM syntax

section .multiboot_header align=8

multiboot_header_start:
    dd 0xE85250D6          ; magic number
    dd 0                   ; architecture (0 for i386)
    dd multiboot_header_end - multiboot_header_start ; header length
    dd -(0xE85250D6 + 0 + (multiboot_header_end - multiboot_header_start)) ; checksum

    ; Tag: information request
    align 8
info_request_tag_start:
    dw 1                   ; type = 1
    dw 0                   ; flags
    dd info_request_tag_end - info_request_tag_start ; size
    dd 4                   ; request memory info
    dd 6                   ; request memory map
    dd 8                   ; request framebuffer info
info_request_tag_end:

    ; Tag: framebuffer (1024x768x32)
    align 8
    dw 5                   ; type = 5 (framebuffer)
    dw 0                   ; flags
    dd 20                  ; size
    dd 1024                ; width
    dd 768                 ; height
    dd 32                  ; depth

    ; Tag: end
    align 8
    dw 0                   ; type = 0
    dw 0                   ; flags
    dd 8                   ; size
multiboot_header_end:
