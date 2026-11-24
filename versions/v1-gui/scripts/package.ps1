# RazerMate 免安装打包脚本（Windows PowerShell 5.1+）
#
# 用法：在任意目录打开 PowerShell，执行
#     powershell -ExecutionPolicy Bypass -File versions\v1-gui\scripts\package.ps1
# 产物在 versions\v1-gui\dist\RazerMate\，整个目录拷走即可，不需要安装。

$ErrorActionPreference = 'Stop'
# 脚本位于 versions/v1-gui/scripts/，往上三级才是 Cargo workspace 根。
$root = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$versionDir = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    Write-Host '构建 release ...'
    # -j 8 是实测值：本机 16 核 32 线程跑满 32 并行时，rustc 编译 zune-jpeg
    # 会出现 0xC0000005 段错误；8 并行稳定，且仍然充分占用多核。
    & cargo build --release --workspace -j 8
    if ($LASTEXITCODE -ne 0) {
        throw "cargo build 失败（退出码 $LASTEXITCODE）"
    }

    $dist = Join-Path $versionDir 'dist\RazerMate'
    New-Item -ItemType Directory -Force -Path $dist | Out-Null

    $exes = @('razermate.exe', 'razermate-agent.exe', 'razerctl.exe')
    foreach ($exe in $exes) {
        $src = Join-Path $root "target\release\$exe"
        if (-not (Test-Path $src)) {
            throw "缺少产物：$src"
        }
        Copy-Item $src (Join-Path $dist $exe) -Force
    }
    Copy-Item (Join-Path $versionDir 'docs\使用说明.txt') (Join-Path $dist '使用说明.txt') -Force

    Write-Host ''
    Write-Host "完成：$dist"
    foreach ($exe in $exes) {
        $f = Get-Item (Join-Path $dist $exe)
        Write-Host ("  {0,-24} {1:N2} MB" -f $exe, ($f.Length / 1MB))
    }
} finally {
    Pop-Location
}
