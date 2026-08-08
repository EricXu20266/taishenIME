import subprocess, os, sys, glob
os.chdir(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "engine"))
# 找到最新的 ffi_integration 测试 exe
exes = glob.glob(r"target\debug\deps\ffi_integration-*.exe")
exe = max(exes, key=os.path.getmtime)
print("EXE:", os.path.basename(exe), file=sys.stderr)
with open("ffi_run.txt", "wb") as f:
    p = subprocess.run([exe, "--test-threads=1"], stdout=f, stderr=subprocess.STDOUT, timeout=180)
print("EXIT:", p.returncode, file=sys.stderr)
