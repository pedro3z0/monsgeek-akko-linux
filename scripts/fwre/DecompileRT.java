// Decompile the Rapid-Trigger functions and print C, so the reset-point logic can
// be read instead of inferred from disassembly.
//
// Raw firmware: auto-analysis does not necessarily reach these routines, so each
// address is force-disassembled and a function created before decompiling.
// Addresses come from fw_symbols.ld (a prior Ghidra run) since this import has
// no symbol table.
import ghidra.app.script.GhidraScript;
import ghidra.app.cmd.disassemble.DisassembleCommand;
import ghidra.app.cmd.function.CreateFunctionCmd;
import ghidra.app.decompiler.DecompInterface;
import ghidra.app.decompiler.DecompileOptions;
import ghidra.app.decompiler.DecompileResults;
import ghidra.program.model.address.Address;
import ghidra.program.model.listing.Function;
import ghidra.program.model.listing.FunctionManager;
import java.io.BufferedReader;
import java.io.FileReader;

public class DecompileRT extends GhidraScript {
    @Override
    public void run() throws Exception {
        FunctionManager fm = currentProgram.getFunctionManager();
        DecompInterface di = new DecompInterface();
        DecompileOptions opts = new DecompileOptions();
        di.setOptions(opts);
        di.openProgram(currentProgram);

        BufferedReader r = new BufferedReader(new FileReader("/tmp/fwre/addrs.txt"));
        String line;
        while ((line = r.readLine()) != null) {
            String[] p = line.trim().split("\\s+");
            if (p.length < 2) continue;
            String name = p[0];
            long v = Long.parseLong(p[1].replace("0x", ""), 16) & ~1L;
            Address a = currentProgram.getAddressFactory().getDefaultAddressSpace().getAddress(v);

            Function f = fm.getFunctionContaining(a);
            if (f == null) {
                DisassembleCommand dc = new DisassembleCommand(a, null, true);
                dc.applyTo(currentProgram, monitor);
                CreateFunctionCmd cf = new CreateFunctionCmd(a);
                cf.applyTo(currentProgram, monitor);
                f = fm.getFunctionContaining(a);
            }
            if (f == null) { println("### " + name + ": still no function at " + a); continue; }
            println("\n======== " + name + " @ " + a + "  (" + f.getName() + " @ "
                    + f.getEntryPoint() + ", " + f.getBody().getNumAddresses() + " bytes) ========");
            DecompileResults res = di.decompileFunction(f, 120, monitor);
            if (res.decompileCompleted()) println(res.getDecompiledFunction().getC());
            else println("  decompile failed: " + res.getErrorMessage());
        }
        r.close();
        di.dispose();
    }
}
