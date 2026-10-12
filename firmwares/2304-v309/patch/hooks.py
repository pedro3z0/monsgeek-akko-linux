#!/usr/bin/env python3
"""
Hook configuration for FUN60 Pro (dev_id 2304, firmware v309) standard-RT patch.

Goal: implement true "Standard" (Wooting-style) Rapid Trigger with runtime flavor
switching and patch discovery.

Hooks:
  1. rt_eval (0x08001FE8):
     Filter hook on per-key RT evaluation. When g_rt_flavour == 0 (traditional/standard),
     disarms RT whenever key depth rises above actuation (d < act). When g_rt_flavour == 1,
     passes through to stock continuous RT (CRT).
  2. vendor_dispatch (0x08011BF8):
     Filter hook on vendor command dispatch. Handles:
       - 0xE7: GET_PATCH_INFO (returns magic 0xCAFE, version, capability standard_rt)
       - 0x1F: SET_RTMODE (0 = traditional, 1 = continuous)
       - 0x9F: GET_RTMODE (returns current RT flavor)
     Other commands pass through to original firmware handlers.

Usage:
    python3 hooks.py generate    # Write hooks_gen.S
    python3 hooks.py patch       # Compile + splice hook.bin into fw_at32.bin
    python3 hooks.py validate    # Validate hook points only
"""

import sys
from pathlib import Path

# Shared hook framework at repo root
sys.path.insert(0, str(Path(__file__).resolve().parent.parent.parent.parent / "patch"))
from hook_framework import BinaryPatch, Hook, PatchProject

SCRIPT_DIR = Path(__file__).parent

HOOKS = [
    Hook(
        name="rt_eval",
        target=0x08001FE8,       # rt_eval_per_key
        handler="handle_rt_eval",
        mode="filter",
        displace=4,              # stmdb sp!,{r4-r10,lr} — 4-byte Thumb2, safe
    ),
    Hook(
        name="vendor_dispatch",
        target=0x08011BF8,       # vendor_command_dispatch
        handler="handle_vendor_cmd",
        mode="filter",
        displace=4,              # stmdb sp!,{r4-r10,lr} — 4-byte Thumb2, safe
    ),
]

BINARY_PATCHES = []

project = PatchProject(
    hooks=HOOKS,
    binary_patches=BINARY_PATCHES,
    firmware_bin=SCRIPT_DIR / ".." / "fw_at32.bin",
    patched_bin=SCRIPT_DIR / ".." / "fw_at32_patched.bin",
    hook_bin=SCRIPT_DIR / "hook.bin",
    elf_path=SCRIPT_DIR / "hook.elf",
    build_dir=SCRIPT_DIR,
    engine_kwargs=dict(
        file_base=0x08000000,          # fw_at32.bin starts at flash 0x08000000
        patch_zone_start=0x0801e800,   # first free 2KB sector after code
        patch_zone_end=0x0801ffff,     # 6KB patch budget
    ),
    flash_size=256 * 1024,    # AT32F405RCT7: 256KB flash
    sram_size=96 * 1024,      # 96KB SRAM (0x20000000-0x20017FFF)
)

if __name__ == '__main__':
    project.main()
