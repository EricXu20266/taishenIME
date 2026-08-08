# Auto: open settings -> click OK -> check crash
$exe = "E:\AllinDeepSeek\taishenIME\platform\windows\out\test_settings_dialog.exe"
$proc = Start-Process -FilePath $exe -PassThru
Start-Sleep -Seconds 3
$p = Get-Process -Id $proc.Id -ErrorAction SilentlyContinue
$hwnd = $p.MainWindowHandle
if ($hwnd -eq 0) { "no main window"; exit }
"hwnd=$hwnd"

Add-Type @"
using System;
using System.Runtime.InteropServices;
public class W2 {
    [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
    [DllImport("user32.dll")] public static extern bool PostMessageW(IntPtr h, uint m, IntPtr w, IntPtr l);
    [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
    public struct RECT { public int L, T, R, B; }
}
"@
$r = New-Object W2+RECT
[W2]::GetWindowRect([IntPtr]$hwnd, [ref]$r) | Out-Null
"win: $($r.L),$($r.T) - $($r.R),$($r.B)"

# Click OK: footer buttons right-aligned (open/default/ok/cancel). OK left of Cancel.
# WM_LBUTTONDOWN lParam = CLIENT coords (relative to window origin)
$cx = ($r.R - 122) - $r.L
$cy = ($r.B - 24) - $r.T
"click OK at client ($cx,$cy)"
[W2]::PostMessageW([IntPtr]$hwnd, 0x0201, [IntPtr]0, [IntPtr](($cy -shl 16) -bor ($cx -band 0xFFFF))) | Out-Null
Start-Sleep -Milliseconds 150
[W2]::PostMessageW([IntPtr]$hwnd, 0x0202, [IntPtr]0, [IntPtr](($cy -shl 16) -bor ($cx -band 0xFFFF))) | Out-Null
Start-Sleep -Seconds 2
"config.ini mtime: $((Get-Item 'E:\AllinDeepSeek\taishenIME\platform\windows\out\config.ini').LastWriteTime)"
$proc.Refresh()
if ($proc.HasExited) {
    "RESULT: EXITED code=$($proc.ExitCode) (0=normal, nonzero=crash)"
} else {
    "RESULT: ALIVE (no crash)"
    Stop-Process -Id $proc.Id -Force -Confirm:$false
}
