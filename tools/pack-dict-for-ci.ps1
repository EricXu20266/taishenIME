# 打包词库产物，供 GitHub Actions 构建时下载使用
#
# 背景：resources/ 下的词库全部是 taishen-dict 的构建产物（gitignore），
# CI 检出仓库后拿不到，必须从外部下载。本脚本把当前机器上已同步好的
# 词库打成 zip，上传到 taishen-dict（或 taishenIME）的 Release，
# 再把下载地址填进仓库变量 DICT_ASSET_URL。
#
# 用法：
#   .\tools\pack-dict-for-ci.ps1                     # 输出 dist\taishen-dict-asset.zip
#   .\tools\pack-dict-for-ci.ps1 -OutFile D:\x.zip   # 指定输出路径

param(
    [string]$OutFile = ""
)

$ErrorActionPreference = "Stop"
$repoRoot = Split-Path -Parent $PSScriptRoot
$resDir = Join-Path $repoRoot "resources"

if (-not (Test-Path $resDir)) {
    throw "未找到 resources 目录: $resDir"
}

# 必需项：缺任何一个都说明词库未同步完整
$required = @(
    "system_dict.db",
    "VERSION.json",
    "common.db",
    "bigram.db"
)
foreach ($name in $required) {
    $p = Join-Path $resDir $name
    if (-not (Test-Path $p)) {
        throw "词库缺失: $p（先运行 taishen-dict 的 tools\sync_to_ime.py）"
    }
}

# 可选项：引擎运行时按需加载，缺了不影响构建
$optional = @(
    "system_dict.db.bin",
    "domains\domains.db",
    "domains\domains.db.bin",
    "lm\lm_cache.bin",
    "lm\lm_data.bin",
    "lm\lm_words.json"
)

if (-not $OutFile) {
    $distDir = Join-Path $repoRoot "dist"
    New-Item -ItemType Directory -Path $distDir -Force | Out-Null
    $OutFile = Join-Path $distDir "taishen-dict-asset.zip"
}

$staging = Join-Path $env:TEMP ("dict-stage-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Path $staging -Force | Out-Null

try {
    $staged = @()
    foreach ($name in $required) {
        $src = Join-Path $resDir $name
        $dst = Join-Path $staging $name
        New-Item -ItemType Directory -Path (Split-Path -Parent $dst) -Force | Out-Null
        Copy-Item $src $dst -Force
        $staged += $name
    }
    foreach ($name in $optional) {
        $src = Join-Path $resDir $name
        if (Test-Path $src) {
            $dst = Join-Path $staging $name
            New-Item -ItemType Directory -Path (Split-Path -Parent $dst) -Force | Out-Null
            Copy-Item $src $dst -Force
            $staged += $name
        } else {
            Write-Host "[WARN] 跳过缺失的可选项: $name"
        }
    }

    if (Test-Path $OutFile) { Remove-Item $OutFile -Force }
    Compress-Archive -Path (Join-Path $staging '*') -DestinationPath $OutFile -CompressionLevel Optimal

    $sizeMb = [math]::Round((Get-Item $OutFile).Length / 1MB, 1)
    Write-Host ""
    Write-Host "[OK] 词库包: $OutFile ($sizeMb MB)" -ForegroundColor Green
    Write-Host "     打包内容: $($staged -join ', ')"
    Write-Host ""
    Write-Host "下一步：" -ForegroundColor Yellow
    Write-Host "  1) 上传到 taishen-dict 的 Release："
    Write-Host "     gh release create dict-<版本> `"$OutFile`" -R EricXu20266/taishen-dict -t `"词库产物 <版本>`" -n `"CI 构建用词库产物`""
    Write-Host "  2) 把下载地址填进 taishenIME 仓库变量 DICT_ASSET_URL"
} finally {
    Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
}
