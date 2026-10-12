/* Linker script for FUN60 Pro (dev_id 2304, fw v309) standard-RT binary patch.
 * Patch zone: free flash after firmware code ends at 0x0801e6fc.
 * Next 2KB-aligned sector: 0x0801e800.  AT32F405RCT7 has 256KB flash total.
 * We reserve 16KB for patch code + 4KB SRAM for BSS/state. */
MEMORY {
    PATCH     (rx)  : ORIGIN = 0x0801e800, LENGTH = 16384
    PATCH_SRAM (rw) : ORIGIN = 0x20009800, LENGTH = 4096
}
SECTIONS {
    .text : {
        *(.text*)
    } > PATCH
    .rodata : {
        *(.rodata*)
    } > PATCH
    .bss (NOLOAD) : {
        __patch_bss_start = .;
        *(.rtt*)
        *(.bss*)
        *(COMMON)
        __patch_bss_end = .;
    } > PATCH_SRAM
    /DISCARD/ : {
        *(.ARM.*)
        *(.comment)
        *(.note*)
        *(.data*)
    }
}
