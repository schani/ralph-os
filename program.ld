/* Ralph OS Program Linker Script */
/* Programs are position-independent and can be loaded at any address */

ENTRY(_start)

SECTIONS
{
    /* Start at 0 - actual load address is determined at runtime */
    . = 0;

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

    /* End of program */
    _program_end = .;

    /* Discard unnecessary sections */
    /DISCARD/ :
    {
        *(.eh_frame)
        *(.comment)
        *(.note.*)
    }
}
