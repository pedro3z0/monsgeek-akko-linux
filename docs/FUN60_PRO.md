# FUN60 Pro (RY5088) — live-verified notes, firmware v309

[PROTOCOL.md](PROTOCOL.md) is the specification. This file is the model- and
firmware-specific evidence behind it for the **MonsGeek FUN60 Pro**
(VID `0x3151`, PID `0x502d`, device id **2304**, firmware **v309**) — including
the parts that turned out *not* to be true, and the questions that are still
open.

Everything below was read off this exact keyboard over its vendor HID
interface with a small Windows client (`fun60ctl`, stdlib-only Python), not
through the vendor app. Commands are read-only unless marked WRITE and
confirmed by reading the value back. The USB captures referenced in section 3
live outside this repository, next to that client.

**Status:** these are findings, not implemented behaviour.
[Section 8](#8-where-this-stands-in-the-driver) lists what the Rust driver in
this repository does today for each one — several of these quirks are *not*
handled yet, and it says which.

## 1. Transport

- Config interface: USB MI_02, HID usage page `0xFFFF`, usage `0x02`.
  65-byte feature reports = one report-ID byte (`0x00`) + a 64-byte frame.
- Vendor input collection (MI_01 COL05, `0xFFFF`/`0x01`, 32-byte input reports)
  is present; enabling depth reports (`SET_MAGNETISM_REPORT`) was not tested on
  this firmware.
- Framing: `[cmd][params...][checksum][payload...]`
  - Bit7 (all ordinary commands): checksum at frame[7], payload from frame[8].
  - Bit8 (LED params): checksum at frame[8], payload from frame[9].
- Ordinary GETs echo the command byte at frame[0]. Paged data reads (magnetism,
  keymatrix, Fn) return raw data with **no echo**, starting at frame[0].
- Descriptor, read from a USB capture: `bcdUSB 0x0200`, three interfaces
  (EP1 IN `0x81` boot keyboard, EP2 IN `0x82` vendor travel reports, EP3 OUT
  `0x83`), each interrupt, `wMaxPacketSize` 64, `bInterval 1`. With
  `bInterval 1` the endpoint contract is 1 ms at full speed and 125 µs at high
  speed, so the descriptor alone does not say which one was negotiated.
- The key endpoint is **change-only**: it emits a report when the key state
  changes and nothing while a key is held (section 3).

## 2. Identity

- `GET_USB_VERSION` (0x8F) RX: `8F 00 09 00 00 00 00 09 03 …`
  - device id = u32 LE frame[1:5] = **2304**. PID `0x502d` is shared by ~46
    entries in the device database, so the id is the real discriminator.
  - version word = `frame[7] | frame[8] << 8` = **0x0309 = v309**.
  - precision = version ≥ 768 → **0.01 mm steps, raw = mm × 100**.
- `GET_FEATURE_LIST` (0xE6) returns all zeros on v309 — it is a stub. Never
  take the precision from it; use the version word.
- Commands that merely echo back on stock v309: 0x80, 0x9D, 0xD0, 0xAE, 0xAD,
  0xE7.
- `GET_SLEEPTIME` (0x91) does return real data: `2C 01 2C 01 90 06 90 06`
  (300, 300, 1680, 1680 s — the exact field split is unresolved).

## 3. Polling rate: what is actually known

Three levels of evidence, kept apart on purpose.

**3.1 The setting round-trips.** `SET_REPORT` (0x03) with `[0x00, code]` is
accepted, and `GET_REPORT` (0x83) echoes the new code after roughly 400 ms —
read sooner and the *previous* code comes back. 4000 → 8000 Hz was accepted and
read back as code 0. Verdict: the firmware stores the setting, and that is all
this shows. Codes: 0 = 8000, 1 = 4000, 2 = 2000, 3 = 1000, 4 = 500, 5 = 250,
6 = 125 Hz. 8000 Hz is this model's documented maximum (`reportRate` in the
device database).

**3.2 The travel stream does not track it.** Reading the vendor input
collection while keys move, at three settings, 5 s each:

| setting | reports | median gap | implied |
|---|---|---|---|
| 8000 Hz | 12123 | 126 µs | ~7949 Hz |
| 4000 Hz | 11791 | 126 µs | ~7949 Hz |
| 1000 Hz | 11490 | 126 µs | ~7930 Hz |

Flat across a 64× range. The 126 µs is the **host-side read loop**, not the
wire: the host can pull the buffered vendor report that fast, while the device
only puts new data on the wire when something changes. This measures how fast
Python can spin, not the polling schedule.

**3.3 Boot-keyboard reports are the channel that matters, and cannot be
timed.** The boot collection (MI_00, col03) cannot be opened directly on
Windows — `ERROR_ACCESS_DENIED`, because the OS keyboard stack owns it — so
reports were counted through the Raw Input API instead, with a plumbing test
(25 injected key events → 50/50 records received) proving the capture path
works. One key held or tapped per run, 8 s:

| setting | reports / 8 s | rate | median gap |
|---|---|---|---|
| 8000 Hz | 81 | ~10/s | 69 ms |
| 4000 Hz | 82 | ~10/s | 66 ms |
| 1000 Hz | 77 | ~10/s | 90 ms |
| 8000 Hz | 74 | ~9/s | 99 ms |
| 8000 Hz | 91 | ~11/s | 66 ms |
| 125 Hz | 85 | ~11/s | 66 ms |

The machine's own typematic reference is ~30 repeats/s, so these are neither a
device stream nor OS auto-repeat — they match hand tapping (~5 taps/s ×
press+release). Inconclusive by construction: with a key merely held, the
interface emits nothing at all.

**3.4 USB capture ground truth.** A USBPcap capture of the hub carrying
VID 0x3151 settled the mechanism: **USBPcap only logs data-bearing interrupt
transfers**. Empty polls (the device NAKs when no key state changed) are not
captured, so packet counts are key events, never a poll rate.

| capture | endpoint | data frames | meaning |
|---|---|---|---|
| 8000 Hz, 6 s | 0x81 (8 B) | `00…00` @0.00 s, `00 00 04 …` @0.14 s, `00…00` @4.49 s, `00 00 04 …` @4.60 s | two press/release pairs (`04` = keycode A, third byte) |
| 1000 Hz, 6 s | 0x81 (8 B) | `00…00` @0.00 s, `00 00 04 …` @0.20 s | a press at the start, release shortly after |

Press and release are 100–200 ms apart at both settings — normal human tapping,
the same shape at 8000 and 1000 Hz. A held key produces one press record and
one release record with no stream in between, so counting packets can never
measure polling on this board. What the setting most plausibly gates is the
internal scan pipeline (analog sampling and debounce feeding EP1 and the EP2
travel reports), and neither observable channel moved with it.

**Bottom line.** The setting is stored and round-trips; no wire effect has been
demonstrated on fw v309. Leave it at 8000 Hz — the advertised maximum, and
harmless — but treat the number as unverified firmware configuration rather
than a measured rate. The levers that demonstrably matter on this board are
debounce (already 0 ms) and RT sensitivity.

**If this is ever retried:** hold one key *perfectly still* for the whole window
and compare the measured rate with the OS repeat rate. If the measured value
follows the OS setting, the test is measuring Windows, not the keyboard; 0–2
records means the board streams nothing (change-only); hundreds or thousands
per second would be a real device stream, and only then does a `--rate`
comparison mean anything.

**3.5 There is no turbo or scan-mode command.** No such byte exists anywhere in
the vendor protocol, the KBOPTION payload, the device database or the key-code
table. The levers that do exist are polling rate, debounce (0x06) and the RT
stability gate.

## 4. Simple settings (all verified)

| setting | GET / SET | layout | notes |
|---|---|---|---|
| Profile | 0x84 / 0x04 | frame[1] | 4 profiles |
| Polling | 0x83 / 0x03 | code at **frame[2]** | SET `[0x00, code]`; frame[1] is reserved and reads 0, so reading there makes every rate look like the 8 kHz code (see 3.1) |
| Debounce | 0x86 / 0x06 | frame[1], ms | currently 0 |
| LED on/off | 0x85 / 0x05 | frame[1] | polarity looks inverted (0 = on); WRITE polarity unconfirmed, needs a see-it-with-your-eyes test |
| LED params | 0x87 / 0x07 (Bit8) | `[mode, speed_raw, brightness, option, r, g, b]` | user speed = `4 - speed_raw`; current mode 1 (constant), rgb ≈ white |
| KBOPTION | 0x89 / 0x09 | `[os, fn_layer, anti_mistouch, rt_stability, wasd_swap]` | all zero on this board |

- Two LED readings are open questions on v309: brightness reads 0 while the
  board is clearly lit, and the option low nibble reads `0x07` ("dazzle" in the
  usual decode) while the LEDs are a static white. Take both with salt.
- `rt_stability` is the only anti-jitter knob for Rapid Trigger: level 0–5,
  25 ms per step. This board runs at 0, i.e. with no stability gating.
- LED modes named so far: 0 off, 1 constant, 2 breathing, 3 neon, 4 wave,
  5 ripple, 6 raindrop, 7 snake, 8 reactive, 9 converge, 10 sine,
  11 kaleidoscope, 12 line-wave, 13 user-picture, 14 laser, 15 circle-wave,
  16 rainbow, 17 rain-down, 18 meteor, 19 reactive-off, 20 music-patterns,
  21 screen-sync, 22 music-bars, 23 train, 24 fireworks, 25 per-key-color.
  Mode 13 is the exception: it carries its layer in the option high nibble and
  ignores the colour.

## 5. Magnetism (0x65 SET / 0xE5 GET)

Every layout below was confirmed by writing and reading it back.

Read: `[0xE5, sub, 0x01, page]` → 64 raw bytes per page.

| width | sub-commands | pages |
|---|---|---|
| u8 | 0x07 key mode, 0x09 snap-tap enable, 0x05 mod-tap time | 1 |
| u16 LE | 0x00 press, 0x01 lift, 0x02 rt_press, 0x03 rt_lift, 0x04 dks_travel, 0x06 bottom deadzone, 0xFB top deadzone | 2 |
| — | 0x0A dks_modes | 8 (not decoded here) |

- **Top deadzone (0xFB) is junk on v309.** It reads like `64485`, `7424` —
  treat it as unsupported even though the sub-command echoes normally.
- Sub-command 0x04 is worth a second look: on this board `dks_travel` read back
  as one u16 per key over 2 pages, and the driver's `get_dks_travels` agrees.
  [PROTOCOL.md](PROTOCOL.md) section 3.5 lists 0x04 as 4-byte values, 16 pairs
  per page, which contradicts both — resolve that before DKS travel is written
  from either description.
- Write, paged (u16 tables): `[0x65, sub, 1, page, commit]` + up to 56 bytes of
  payload, with `commit = 1` on the **last page only**. Around 30 ms between
  pages, around 250 ms settle after the commit.
- Write, per key: `[0x65, sub, 0, key_index, commit]` + payload. This mirrors
  the vendor web app's own `_sendMagnetismInfoSimpleCMD`, and is the path used
  for every mode and snap-tap write here.

## 6. Modes and the RT/CRT question

- The vendor app's own encoder is `base | 128` when "fire" is on: Rapid Trigger
  is the **orthogonal 0x80 flag**, not a base mode, and it combines with any
  base mode.
- Base modes confirmed on this board: 0 normal, 2 dks, 3 mt, 4 tgl_hold,
  5 tgl_dots, 7 snap. Anything else renders as `mode<n>`.
- **Undocumented values are accepted and stored.** Writing raw mode byte `1` to
  key A read back as `1` — but the key then produced **no output at all**
  (user-tested). Value 1 is a key-disabler on v309, not a classic-RT mode, so
  treat unknown values as key-disablers until proven otherwise. Values 1, 6, 8
  and up are undocumented on this firmware.
- Trial 2 (standard-RT mapping) ran on A/W/S/D as `press 1.50 / lift 1.10 /
  rt_press 0.40 / rt_lift 0.40`, mode 128. The rationale: in Wooting-style RT
  the key releases after `sensitivity` mm of lift, i.e.
  `release = actuation - sensitivity`. The vendor preset's `lift 0.50` leaves a
  1.0 mm hover band in which the key re-fires without ever crossing the
  actuation point again, which is CRT behaviour by configuration. Two things had
  to hold for config-only standard RT to be possible:
  1. the key must turn off after ~0.4 mm of lift (not 0.5 mm), and
  2. hovering at ~1.2 mm and pressing must **not** fire until 1.5 mm is crossed
     again.
  If (2) fires while hovering, the firmware re-triggers anywhere and
  config-only standard RT is impossible — the fallback is a host-side layer or a
  firmware patch. This trial was left in place on the board; the feel test is
  what decides it.
- Modes live in the **active profile**. Keymaps are per (profile, layer) with
  4 layers, and the Fn table is a separate store (sys 0/1).

## 7. Reading right after a write, and flaky identity (v309 quirks)

Two firmware behaviours that anything talking to this board has to defend
against:

1. **Stale table reads.** A magnetism read issued too soon after a commit comes
   back *shifted*: the `press` table reads as the value just written to `lift`,
   `rt_lift` reads 0, `modtap` reads minutes. 250 ms after a commit is **not
   enough** on this firmware. Reads only became trustworthy behind a 2 s window
   — and a tool that reads immediately will show you numbers that are fiction,
   then helpfully write those numbers back.
2. **Stale identity frames.** `GET_USB_VERSION` was caught answering once with a
   foreign frame, which silently flips the travel precision between
   0.01 mm/unit and 0.1 mm/unit and therefore misreports every travel value. The
   identity read has to be retried until the device id is known or the version
   word is plausible.

Both also appear in milder form on the settings path: `GET_REPORT` (0x83),
`GET_KBOPTION` (0x89) and `GET_USB_VERSION` (0x8F) can answer with the
*previous* command's response, so a GET is worth retrying until it echoes the
command that was sent.

## 8. Where this stands in the driver

Measured against `79c45f1`. "Not handled" means the repository does not do it
today — this section describes the hardware and the code side by side so the
gap is visible rather than implied.

| finding | driver today | handled |
|---|---|---|
| Read gate after a magnetism commit | `MAGNETISM_SETTLE_MS = 250` (`monsgeek-keyboard/src/lib.rs`) is the delay *inside* the final per-key write (`send_with_delay`). There is no stateful gate, so a read issued from a later call can still land on a shifted table | no |
| Rate read-back after `SET_REPORT` | `set_polling_rate` sends `[0, code]` and returns; `get_polling_rate` reads immediately, inside the window where v309 still serves the previous code | no |
| Identity plausibility | `get_device_id` / `get_version` accept the first frame whose echo matches; a foreign frame that happens to echo can still flip the precision | no |
| Top deadzone (0xFB) | read in `get_all_triggers` (a failed read degrades to zeros, junk values are taken as data) and written by `set_top_deadzone_all`, with no firmware gate | no |
| Precision from the version word | thresholds 1280 / 768 → ×200 / ×100 / ×10 (`monsgeek-transport/src/protocol.rs`, `Precision`) | yes |
| `0x80` RT flag kept orthogonal to the base mode, unknown bases preserved | `KeyMode` / `ModeByte` (`monsgeek-keyboard/src/magnetism.rs`) | yes |
| Polling rate code at frame[2] | `POLLING_RATE_FRAME_OFFSET` (`monsgeek-keyboard/src/lib.rs`) | yes |
| Rate capped at the model's maximum (8000 Hz here) | `reportRate` in the device database | yes |
| KBOPTION layout, including 25 ms RT-stability steps | `KeyboardOptions` (`monsgeek-keyboard/src/settings.rs`) | yes |
| This model's board facts | `data/devices.json` / `data/device_matrices.json`: id 2304, 61 keys, 4 layers, magnetism, 8000 Hz, matrix `ry5088_akko_fun60pro_1m_8k` | yes |

## 9. Rules for not bricking it

- Chunked SETs are unchecked on this family: never send bulk `SET_MACRO` or
  oversized chunks (`macro_id ≥ 16` overflows the stack in the flash saver).
- A restore should always check the device id, commit once per batch, settle,
  and then read everything back — a mismatch is a result, not a warning to
  swallow.
- Bulk writes have to respect the firmware's lack of bounds checking: paged u16
  writes with commit-on-last-page, and per-key writes for mode bytes (the
  vendor app's own path).
- Rule of thumb: **dump before you poke.** The captured dumps for this board,
  including the factory state it shipped with, are in
  `iot_driver_linux/tests/fixtures/fun60pro/`.
