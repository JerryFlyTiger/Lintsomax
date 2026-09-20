set confirm off
set pagination off

# 動態問 rustc 拿 sysroot，不寫死路徑（別人 clone 下來也能用）
python
import subprocess, sys, gdb
try:
    sysroot = subprocess.check_output(["rustc", "--print", "sysroot"]).decode().strip()
    etc = sysroot + "/lib/rustlib/etc"
    sys.path.insert(0, etc)
    gdb.execute("source " + etc + "/gdb_load_rust_pretty_printers.py")
    print("[lintsomax] Rust pretty-printer 已載入")
except Exception as e:
    print("[lintsomax] 載入 Rust pretty-printer 失敗: %s" % e)
end

target remote :1234
