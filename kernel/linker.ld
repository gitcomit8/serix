ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);    /* R + X */
    rodata PT_LOAD FLAGS(4);  /* R */
    data PT_LOAD FLAGS(6);    /* RW */
    dynamic PT_DYNAMIC FLAGS(6); /* RW */
}

SECTIONS
{
    . = 1M;

    .text : ALIGN(4K)
    {
        *(.text .text.*)
        . = ALIGN(4K);
    } :text
    
    . = ALIGN(4K);

    .dynsym : ALIGN(4K) { *(.dynsym) } :rodata
    .gnu.hash : { *(.gnu.hash) } :rodata
    .hash : { *(.hash) } :rodata
    .dynstr : { *(.dynstr) } :rodata
    .rela.dyn : { *(.rela.dyn) } :rodata
    
    .rodata : ALIGN(4K)
    {
        *(.rodata .rodata.*)
    } :rodata
    
    .eh_frame_hdr : { *(.eh_frame_hdr) } :rodata
    .eh_frame : { *(.eh_frame) } :rodata
    
    . = ALIGN(4K);

    .data : ALIGN(4K)
    {
        *(.data .data.*)
    } :data
    
    .dynamic : { *(.dynamic) } :data :dynamic
    .got : { *(.got) *(.got.plt) } :data
    
    . = ALIGN(4K);

    .bss : ALIGN(4K)
    {
        *(.bss .bss.*)
        *(COMMON)
    } :data
}