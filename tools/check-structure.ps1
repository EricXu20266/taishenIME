# 泰深输入法架构边界自检脚本（对应 docs/ARCHITECT-V4.md §13.2）
# 用法：pwsh -NoProfile -File tools\check-structure.ps1 [-StrictLineLimit]

param(
    [switch]$StrictLineLimit
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path $PSScriptRoot -Parent
$failed = 0

Write-Host "=== 泰深架构边界自检 ===" -ForegroundColor Cyan

# 1. engine/ 禁止依赖 Windows crate
$engineToml = Join-Path $repoRoot "engine\Cargo.toml"
$winDeps = Select-String -Path $engineToml -Pattern '\b(windows|winapi|windows-sys)\b'
if ($winDeps) {
    Write-Host "[FAIL] engine/Cargo.toml 含 Windows 依赖：$winDeps" -ForegroundColor Red
    $failed++
} else {
    Write-Host "[OK]   engine/ 无 Windows crate 依赖" -ForegroundColor Green
}

# 2. 词库清单与常用词表一致性
$python = (Get-Command python -ErrorAction SilentlyContinue) ?? (Get-Command python3 -ErrorAction SilentlyContinue)
$sysDb = Join-Path $repoRoot "resources\system_dict.db"
if (-not (Test-Path $sysDb)) {
    Write-Host "[SKIP] resources/system_dict.db 不存在（未同步词库产物），跳过词库一致性校验" -ForegroundColor Yellow
} elseif ($python) {
    & $python.Source (Join-Path $repoRoot "tools\verify_resources.py") (Join-Path $repoRoot "resources\VERSION.json") | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[OK]   resources/VERSION.json 对账通过" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] resources/VERSION.json 对账失败" -ForegroundColor Red
        $failed++
    }
    & $python.Source (Join-Path $repoRoot "tools\verify_common_dict.py") | Out-Null
    if ($LASTEXITCODE -eq 0) {
        Write-Host "[OK]   common_dict.txt 与 common.db 一致" -ForegroundColor Green
    } else {
        Write-Host "[FAIL] common_dict.txt 校验失败" -ForegroundColor Red
        $failed++
    }
}

# 3. platform/windows/src 单文件行数检查（V4 目标 <= 1000，硬阻断上限 1200）
$lineLimit = 1200
$legacyMonoliths = @("tsf_module.cpp", "settings_window.cpp", "taishen_server.cpp", "imm32_ime.cpp")
$oversized = @()
$blocked = @()
Get-ChildItem (Join-Path $repoRoot "platform\windows\src") -Recurse -Include *.cpp,*.h | ForEach-Object {
    $lines = (Get-Content $_.FullName).Count
    if ($lines -gt 1000) {
        $oversized += [pscustomobject]@{ File = $_.Name; Lines = $lines }
    }
    if ($lines -gt $lineLimit) {
        if ($StrictLineLimit -or ($legacyMonoliths -notcontains $_.Name)) {
            $blocked += [pscustomobject]@{ File = $_.Name; Lines = $lines }
        }
    }
}
if ($oversized.Count -gt 0) {
    Write-Host "[INFO] 超 1000 行 C++ 文件（V4 拆分目标）：" -ForegroundColor Yellow
    $oversized | Sort-Object Lines -Descending | Format-Table -AutoSize | Out-String | Write-Host
} else {
    Write-Host "[OK]   platform/windows/src 单文件均 <= 1000 行" -ForegroundColor Green
}
if ($blocked.Count -gt 0) {
    Write-Host "[FAIL] 超 $lineLimit 行 C++ 文件（非历史豁免或启用 -StrictLineLimit）：" -ForegroundColor Red
    $blocked | Sort-Object Lines -Descending | Format-Table -AutoSize | Out-String | Write-Host
    $failed++
}

if ($failed -gt 0) {
    Write-Host "`n[FAIL] 共 $failed 项边界检查失败" -ForegroundColor Red
    exit 1
}
Write-Host "[PASS] 架构边界检查通过" -ForegroundColor Green
exit 0
