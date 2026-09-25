# FUN60 Pro reference dumps (device id 2304, firmware v309)

Five board-state dumps captured from a MonsGeek FUN60 Pro — VID `0x3151`,
PID `0x502d`, device id **2304**, firmware **v309**, precision 0.01 mm
(`raw = mm × 100`) — with a small Windows client (`fun60ctl`).

They are captures, not fixtures this repository generates, and they are kept
here because they are the evidence behind
[docs/FUN60_PRO.md](../../../../docs/FUN60_PRO.md): every trigger value quoted
in those notes, the junk top-deadzone readings, and the shape of a full dump.

**Nothing reads them yet.** They are reference data for a decode test (61 keys,
4 profiles, 4 layers) that does not exist here. They are *not* in this driver's
profile JSON format, and `iot_driver` cannot load them.

| file | captured | profile | rate | contents |
|---|---|---|---|---|
| `fun60pro-initial.json` | 2026-09-24 01:41:39 | 0 | 4000 Hz | the state the board was in on arrival: keymatrix and Fn for all 4 profiles, triggers for all 4 profiles |
| `fun60pro-postfix.json` | 2026-09-24 01:53:48 | 0 | 4000 Hz | triggers for all 4 profiles after a first pass; no keymatrix/Fn |
| `fun60pro-before-rt150-lift140.json` | 2026-09-24 02:15:07 | 0 | 4000 Hz | active profile only, before the standard-RT mapping was applied |
| `fun60pro-rt150-lift140.json` | 2026-09-24 02:19:04 | 0 | 4000 Hz | same keys, `lift` moved to 1.40 mm |
| `fun60pro-standard-rt.json` | 2026-09-24 02:27:50 | 0 | **8000 Hz** | the trial-2 mapping: A/W/S/D at `press 1.50 / lift 1.10 / rt 0.40`, mode 128 |

## What these dumps show

- Matrix positions **9, 14, 15 and 21 are A, W, S and D**
  (`data/device_matrices.json`, entry `12625:20525:2304`), and they are the
  four keys carrying mode byte 128 — the `0x80` Rapid-Trigger flag — in every
  dump here.
- The whole trial sequence sits in one field. `lift` reads 280 (2.80 mm) on
  ordinary keys throughout, and on the RT keys it moves 50 → 140 → 110 across
  `initial` → `rt150-lift140` → `standard-rt`, while `rt_press` / `rt_lift`
  stay at 40 (0.40 mm).
- `snaptap` holds only `0` or `255`. 255 is the unbound sentinel, which is what
  `SNAPTAP_UNBOUND` means in `monsgeek-keyboard`.
- **`top_dz` is junk on this firmware**, visible here as raw data: the dumps
  carry `64485`, `7424`, `7680` and `257` for it. This is the finding in
  section 5 of the notes.
- `modtap` and `dks_travel` are zero for every key on this board, so these
  dumps cannot tell you the width of magnetism sub-command 0x04.
- `feature_list` is a stub on v309, but not literally all zeros: every dump reads
  `E6 00 00 00 00 00 00 19 00 00 00 00` — the `0xE6` echo, an `0x19` at frame[7],
  zeros elsewhere. `precision_byte` and `capabilities` are 0, so a tool that trusts
  this response for precision reports the wrong unit entirely.
- The trigger tables hold **61 entries**, one per key slot, while the matrix
  entry lists 128 positions of which 60 carry a name — the extra slot is not a
  named switch.

## Dump format

| field | meaning |
|---|---|
| `tool`, `created` | who wrote the dump, and when |
| `device` | `pid`, `device_id`, `version`, `version_str`, `precision_factor`, `board` |
| `profile` | active profile when the dump was taken |
| `rate_code`, `rate_hz` | polling rate as read back from the board |
| `debounce` | milliseconds |
| `led_on` | already un-inverted: `true` means the backlight is lit |
| `led` | `mode`, `mode_name`, `speed` (user-facing, 0–4), `speed_raw`, `brightness`, `option`, `direction`, `dazzle`, `rgb` |
| `options` | `os_mode`, `fn_layer`, `anti_mistouch`, `rt_stability`, `rt_stability_ms`, `wasd_swap` |
| `feature_list` | `precision_byte`, `capabilities`, `raw`. On v309 this is a stub: every dump here answers `E6 00 00 00 00 00 00 19 00 …`, with both `precision_byte` and `capabilities` 0 |
| `keymatrix` | `{"<profile>": {"<layer>": "<hex, 4 bytes per key>"}}`; `null` when the dump did not capture it |
| `fn` | `{"<profile>": {"sys0": "<hex>", "sys1": "<hex>"}}`; `null` when not captured |
| `triggers` | `{"<profile>": {…}}` with `modes`, `snaptap`, `modtap`, `press`, `lift`, `rt_press`, `rt_lift`, `dks_travel`, `bottom_dz`, `top_dz`, one entry per key slot |

A table the firmware refused to answer comes back as `null` in the dump with a
sibling `"<name>_error"` string. None of these five dumps hit that path, which
is itself consistent with the notes: every sub-command except top deadzone
answers here — top deadzone answers too, just with nonsense.

## Restoring one

Only the tool that wrote them can put them back:

```
py cli.py restore FILE --yes
```

It checks the dump's device id against the attached board, writes the trigger
tables profile by profile, writes keymatrix and Fn key by key with a single
commit per batch, then reads everything back and reports mismatches (exit
code 2). `fun60pro-initial.json` is the one to start from if this board ever
has to go back to the state it arrived in.

Do not hand-edit a dump and write it back: chunked SETs are unchecked on this
family, and the flash saver overflows on macro ids ≥ 16. The line endings were
normalised to LF when these files were added — that is the only difference from
the files as captured.
