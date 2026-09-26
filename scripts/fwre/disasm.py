#!/usr/bin/env python3
"""Disassemble a named firmware function using the symbol map + capstone.

    python3 scripts/fwre/disasm.py key_rt_custom_mode
    python3 scripts/fwre/disasm.py 0x0800F1FC 0x180
"""
import re, sys, struct
from capstone import Cs, CS_ARCH_ARM, CS_MODE_THUMB, CS_MODE_LITTLE_ENDIAN

FW = 'firmwares/2949-v408/firmware_2949_v408.bin'
SYM = 'firmwares/2949-v408/patch/fw_symbols.ld'
BASE = 0x200   # file offset of address 0x08000000 (AT32F405 header)

def symbols():
    out = {}
    for line in open(SYM, encoding='utf-8', errors='replace'):
        m = re.match(r'\s*(\S+)\s*=\s*(0x[0-9a-fA-F]+);', line)
        if m:
            out[m.group(1)] = int(m.group(2), 16)
    return out

def code(addr, length):
    d = open(FW, 'rb').read()
    off = (addr - 0x08000000) + BASE
    return d[off:off+length]

def disasm(addr, length):
    md = Cs(CS_ARCH_ARM, CS_MODE_THUMB | CS_MODE_LITTLE_ENDIAN)
    md.detail = True
    syms = symbols()
    rev = {}
    for n, a in syms.items():
        rev.setdefault(a & ~1, []).append(n)
    for i in md.disasm(code(addr, length), addr & ~1):
        tag = ''
        tgt = None
        for op in i.operands:
            if op.type == 2:  # IMM
                tgt = op.imm
        if tgt and (tgt & ~1) in rev:
            tag = '  <' + ','.join(rev[tgt & ~1]) + '>'
        print(f'{i.address:08x}: {i.mnemonic:8s} {i.op_str}{tag}')

if __name__ == '__main__':
    if sys.argv[1].startswith('0x'):
        a = int(sys.argv[1], 16) & ~1
        n = int(sys.argv[2], 0) if len(sys.argv) > 2 else 0x100
    else:
        s = symbols()
        if sys.argv[1] not in s:
            print(f'unknown symbol; {len(s)} available'); sys.exit(1)
        a = s[sys.argv[1]] & ~1
        # run to the next symbol
        nxt = [v & ~1 for v in s.values() if (v & ~1) > a]
        n = (min(nxt) - a) if nxt else 0x200
    disasm(a, min(n, 0x600))
