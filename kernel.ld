/* Ralph OS Kernel Linker Script */
/* Kernel is loaded at 1MB (0x100000) */

ENTRY(_start)

SECTIONS
{
    . = 0x100000;

    /* Code section */
    .text ALIGN(4K) :
    {
        *(.text.boot)       /* Entry point first */
        *(.text .text.*)
    }

    /* Read-only data */
    .rodata ALIGN(4K) :
    {
        *(.rodata .rodata.*)
    }

    /* Initialized data */
    .data ALIGN(4K) :
    {
        *(.data .data.*)
    }

    /* Uninitialized data (zeroed) */
    .bss ALIGN(4K) :
    {
        __bss_start = .;
        *(.bss .bss.*)
        *(COMMON)
        __bss_end = .;
    }

    /* End of kernel */
    __kernel_end = .;

    /* Discard unnecessary sections */
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
        *(.note.*)
    }
}
