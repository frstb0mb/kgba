ENTRY(vector)

SECTIONS {
    . = 0;

    .text : {
        KEEP(*(.text.vector));
        *(.text .text.*);
        *(.rodata .rodata.*);
        . = 0x4000;
    }

    /DISCARD/ : {
        *(.ARM.attributes);
        *(.comment);
        *(.note*);
    }
}
