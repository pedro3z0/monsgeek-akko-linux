# Firmware RE: what is known, and what is not

Work log for answering one question: **does v309's Rapid Trigger use a fixed reset
point (standard RT) or a relative lift (CRT)?** The behavioural tests were
inconclusive, so this is being settled in the code.

## The shipped FUN60 Pro firmware cannot be analysed

Queried the vendor API and pulled the real image:

```
POST https://api2.rongyuan.tech:3816/api/v2/get_fw_version  {"dev_id": 2304}
  → "file_path": "fw_upgrade_file/2304_v309"
```

| measurement | value |
|---|---|
| size | 49,431 bytes |
| entropy | **7.989 bits/byte** |
| distinct 32-byte blocks | 1544 of 1544 (100%) |
| non-0xFF/0x00 runs | no structure |

Encrypted, not compressed. `iot_driver firmware validate` accepts it and
`firmware flash` would send it **verbatim** — there is no decrypt step in the
driver. So the decryption key and algorithm live in the device's bootloader /
OTA engine. Static analysis needs a flash dump, i.e. SWD.

## What *is* analysable

`firmwares/2949-v407/dfu_dumps/` holds decrypted DFU dumps of the **M1 V5**
(device 2949) — same vendor family, same AT32F405 Cortex-M4F.

- `firmwares/2949-v407/ghidra_v407.gpr` / `.rep` — **a 38 KB stub, not a real
  analysed database.** It contains no analysis to reuse.
- `firmwares/2949-v408/patch/fw_symbols.ld` — 2131 symbols, but see below.

Images begin with the ASCII chip ID `AT32F405`; the real vector table is at file
offset **0x200**, so `file_offset = address - 0x08000000 + 0x200`.

## The symbol map is unreliable — do not trust the names

`fw_symbols.ld` came from a **v407** analysis but is being applied to the
**v408** image. Disassembling and decompiling at those addresses shows the names
are wrong or land mid-function:

| symbol | address | what is actually there |
|---|---|---|
| `key_rt_custom_mode` | 0x08010104 | 44-byte helper: sets/clears bit 2 of a mode word, writes 0xff |
| `key_per_switch_state_machine` | 0x08010AA4 | 8-byte helper: `state[0x20] \|= arg` |
| `key_actuation_eval` | 0x0800F1FC | its real entry is ~0x0800F10C; the body is **HID report assembly** (ORs 0x20000000/0x04000000/0x80000000 flags, copies report words) — not trigger evaluation |

So "the RT function is at address X" cannot be taken from this file. Locating it
has to be done structurally: find the code that indexes the per-key trigger
struct and compares the measured depth against the stored actuation and release
fields.

## Tooling (works, no further setup needed)

- `disasm.py <symbol|0xADDR> [len]` — capstone, symbol-aware, resolves call targets
- `DecompileRT.java` — Ghidra headless postscript; reads `name addr` pairs from
  `/tmp/fwre/addrs.txt`, force-disassembles, creates functions, decompiles
- Invocation that works:
  ```
  analyzeHeadless <projdir> <name> -import firmware.bin \
    -loader BinaryLoader -loader-baseAddr 0x08000000 \
    -processor ARM:LE:32:Cortex -noanalysis \
    -scriptPath scripts/fwre -postScript DecompileRT.java
  ```
  Use a **fresh project**; re-processing an existing one inherits stale
  function boundaries and silently truncates results. Ghidra here has no Jython —
  postscripts must be Java.

## Where the structural search stands

The command dispatch is a **jump table**, not a comparison chain: a linear
disassembly of the whole image finds **zero** `cmp` instructions against
`0x65` (`SET_MULTI_MAGENTISM`) or `0xE5` (`GET_MULTI_MAGENTISM`). The handler is
reached by indexing a table with the command byte, so it has to be found by
reversing the table dispatch, not by pattern-matching constants.

An inference worth recording, from the data model rather than from RT: the
firmware stores **two** separate sensitivities per key — `rt_press` (sub-command
0x02) and `rt_lift` (0x03) — on top of the actuation (0x00) and release (0x01)
points. Standard RT needs only a reset point; press/lift *sensitivities* are the
CRT shape, since they describe how far to move rather than where to return to.
That is suggestive, not proof: the firmware could still gate on the release
field and use the sensitivities for something else. Settling it needs the code.

## What the remaining RT question is worth

It is the difference between:
- **fixed-point** — a config that works at Wooting-like settings, and
- **CRT** — needing either a firmware patch or a much deeper trigger point.

Behaviourally, `W A S D` at actuation 1.5 / release 0.7 / rt_lift 0.8 is
configured and verified for clean single triggers with a real re-arm, which is
usable for counter-strafing either way. The open question is only whether a
shallower trigger is achievable, and that is what the firmware decides.
