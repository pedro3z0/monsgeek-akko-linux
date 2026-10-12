PREFIX  := arm-none-eabi-
CC      := $(PREFIX)gcc
AS      := $(PREFIX)gcc
LD      := $(PREFIX)ld
OBJCOPY := $(PREFIX)objcopy
OBJDUMP := $(PREFIX)objdump

SHARED  := ../../../patch
CFLAGS  := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -Os -nostdlib -ffreestanding -Wall \
           -I$(SHARED)
ASFLAGS := -mcpu=cortex-m4 -mthumb -mfloat-abi=hard -mfpu=fpv4-sp-d16 \
           -nostdlib -ffreestanding

LDSCRIPT := patch.ld
FIRMWARE := ../fw_at32.bin

# ── Framework-based build ────────────────────────────────────────────────────
HOOK_OBJS := hooks_gen.o handlers.o

.PHONY: all clean patch disasm validate

all: hook.bin

# Generate stubs from hook definitions
hooks_gen.S: hooks.py $(SHARED)/hook_framework.py $(FIRMWARE)
	python3 hooks.py generate

%.o: %.S
	$(AS) $(ASFLAGS) -c $< -o $@

# C handlers compiled separately
handlers.o: handlers.c
	$(CC) $(CFLAGS) -c handlers.c -o $@

hook.elf: $(HOOK_OBJS) $(LDSCRIPT)
	$(LD) -T $(LDSCRIPT) $(HOOK_OBJS) -o $@

hook.bin: hook.elf
	$(OBJCOPY) -O binary $< $@

disasm: hook.elf
	$(OBJDUMP) -d $<

validate: $(FIRMWARE)
	python3 hooks.py validate

# Apply trampolines to firmware
patch: hook.bin $(FIRMWARE)
	python3 hooks.py patch

clean:
	rm -f $(HOOK_OBJS) hook.elf hook.bin hooks_gen.S ../fw_at32_patched.bin
