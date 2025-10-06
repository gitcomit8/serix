ENTRY(_start)

SECTIONS
{
    . = 1M;

    /* Ensure multiboot header is at the beginning */
    .multiboot ALIGN(8) :
    {
        *(.multiboot_header)
    }

    .text ALIGN(4K) :
    {
        *(.text .text.*)
    }

    .rodata ALIGN(4K) :
    {
        *(.rodata .rodata.*)
    }

    .data ALIGN(4K) :
    {
        *(.data .data.*)
    }

    .bss ALIGN(4K) :
    {
        *(COMMON)
        *(.bss .bss.*)
    }

    /* Discard debug information */
    /DISCARD/ :
    {
        *(.comment)
        *(.debug*)
    }
}
