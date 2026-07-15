$ErrorActionPreference = 'Stop'

if (-not $IsWindows) {
    Write-Output 'context-probe: non-Windows platform, skipped.'
    exit 0
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$sourceDir = Join-Path $repoRoot 'src-tauri\context-probe'
$buildDir = Join-Path $repoRoot 'src-tauri\target\context-probe-build'
$binaryDir = Join-Path $repoRoot 'src-tauri\binaries'
$outputPath = Join-Path $binaryDir 'context-probe-x86_64-pc-windows-msvc.exe'

New-Item -ItemType Directory -Path $buildDir -Force | Out-Null
New-Item -ItemType Directory -Path $binaryDir -Force | Out-Null

$configureArgs = @('-S', $sourceDir, '-B', $buildDir, '-G', 'Visual Studio 17 2022', '-A', 'x64')
& cmake @configureArgs
if ($LASTEXITCODE -ne 0) { throw "CMake configure failed with exit code $LASTEXITCODE" }

$buildArgs = @('--build', $buildDir, '--config', 'Release', '--target', 'context-probe')
& cmake @buildArgs
if ($LASTEXITCODE -ne 0) { throw "CMake build failed with exit code $LASTEXITCODE" }

$builtPath = Join-Path $buildDir 'bin\context-probe.exe'
if (-not (Test-Path -LiteralPath $builtPath)) { throw "context-probe output not found: $builtPath" }
Copy-Item -LiteralPath $builtPath -Destination $outputPath -Force
Write-Output "context-probe ready: $outputPath"
