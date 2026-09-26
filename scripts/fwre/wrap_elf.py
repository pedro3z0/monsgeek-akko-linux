"""Wrap raw bytes in a minimal ELF32 so llvm-objdump will disassemble them."""
import struct, sys

def wrap(code, vaddr, out, machine=40, flags=0x5000200):
    ehsize, phentsize, shentsize, nsec = 52, 32, 40, 3
    code_off = ehsize + phentsize + shentsize * nsec
    shstr = b'\0.text\0.shstrtab\0'
    shstr_off = code_off + len(code)
    shoff = shstr_off + len(shstr)   # section headers follow the data

    ph = struct.pack('<IIIIIIII', 1, code_off, vaddr, vaddr, len(code), len(code), 5, 0x1000)
    sh_null = b'\0' * shentsize
    sh_text = struct.pack('<IIIIIIIIII', 1, 1, 0x6, vaddr, code_off, len(code), 0, 0, 4, 0)  # SHF_ALLOC|SHF_EXECINSTR
    sh_str = struct.pack('<IIIIIIIIII', 7, 3, 0, 0, shstr_off, len(shstr), 0, 0, 1, 0)
    eh = b'\x7fELF' + bytes([1, 1, 1, 0, 0]) + b'\0' * 7
    eh += struct.pack('<HHIIIIIHHHHHH', 2, machine, 1, vaddr, code_off, shoff, flags,
                      ehsize, phentsize, 1, shentsize, nsec, 2)
    with open(out, 'wb') as f:
        f.write(eh + ph + sh_null + sh_text + sh_str + code + shstr + sh_null + sh_text + sh_str)

if __name__ == '__main__':
    src, dst, vaddr = sys.argv[1], sys.argv[2], int(sys.argv[3], 0)
    wrap(open(src, 'rb').read(), vaddr, dst)
    print(f'{dst}: wrapped at {vaddr:#x}')
