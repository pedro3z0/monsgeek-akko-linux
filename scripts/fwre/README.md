# Firmware reverse-engineering helpers

Tools for disassembling the RongYuan/AT32F405 keyboard firmware without Ghidra.

## Why this exists

The firmware the vendor serves is encrypted (measured: 7.99 bits/byte entropy,
no repeating 32-byte blocks), and the driver flashes it verbatim — so decryption
happens on the device, inside its bootloader. Static analysis of the shipped file
is therefore a dead end without a flash dump.

What *is* readable is the decrypted dump already in `firmwares/` (DFU dumps from
the M1 V5, device 2949), plus `firmwares/2949-v408/patch/fw_symbols.ld`, a
2131-symbol map from a prior Ghidra run. The MCU is an Artery **AT32F405**, ARM
**Cortex-M4F** @ 216 MHz, so it is Thumb code and `llvm-objdump` disassembles it
natively — no Ghidra, no Java, no radare2 required.

## wrap_elf.py

`llvm-objdump` refuses raw binaries, so this wraps a byte range in a minimal
ELF32 that it will disassemble with correct addresses.

```bash
# extract the symbol's bytes, then wrap and disassemble
python3 - <<'PY'
d = open('firmwares/2949-v408/firmware_2949_v408.bin','rb').read()
BASE = 0x200          # file offset of address 0x08000000 (AT32F405 header)
addr = 0x0800F1FC      # key_actuation_eval, Thumb bit cleared
off  = (addr - 0x08000000) + BASE
open('/tmp/fn.bin','wb').write(d[off:off+0x180])
PY
python3 scripts/fwre/wrap_elf.py /tmp/fn.bin /tmp/fn.elf 0x0800F1FC
llvm-objdump -d --triple=thumbv7em-none-eabi /tmp/fn.elf
```

## Locating the image base

These images begin with the ASCII chip ID `AT32F405`, not a vector table. The
real vector table is at file offset **0x200**; scan for it with:

```python
import struct
for off in range(0, len(d)-8, 4):
    sp, pc = struct.unpack_from('<II', d, off)
    if 0x20000000 <= sp <= 0x20010000 and (pc & 1) and 0x08000000 <= (pc & ~1) < 0x08200000:
        print(hex(off), hex(sp), hex(pc))   # -> base = 0x08000000 - off
```

## Symbols worth knowing

| symbol | address | why |
|---|---|---|
| `key_actuation_eval` | 0x0800F1FC | trigger evaluation |
| `key_rt_custom_mode` | 0x08010104 | Rapid Trigger path |
| `get_multi_magnetism` | 0x08006211 | reads the key table |
| `hid_key_release` | 0x08007839 | release path |
| `send_depth_monitor_report` | 0x08012805 | the depth stream the driver reads |

## Scope

The decrypted dump in `firmwares/` is the **M1 V5**, a different board from the
FUN60 Pro. Both run the same vendor firmware family on the same MCU, so the RT
logic is *probably* shared, but that is an assumption: the two builds have
different addresses and are not interchangeable as a source of truth for the
FUN60 Pro. Confirming it needs a decrypted FUN60 Pro flash, which needs SWD.
