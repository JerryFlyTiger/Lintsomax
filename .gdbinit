set confirm off
set pagination off

# Ask rustc for the sysroot instead of hardcoding a path, so a fresh clone works
python
import subprocess, sys, gdb
try:
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"]).decode().strip()
    etc = sysroot + "/lib/rustlib/etc"
    sys.path.insert(0, etc)
    gdb.execute("source " + etc + "/gdb_load_rust_pretty_printers.py")
    print("[lintsomax] Rust pretty-printer loaded")
except Exception as e:
    print("[lintsomax] failed to load Rust pretty-printer: %s" % e)
end

target remote :1234
