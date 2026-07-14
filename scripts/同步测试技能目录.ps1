param(
    [string]$TestRoot
)

$repositoryRoot = (Resolve-Path (Join-Path $PSScriptRoot "..")).Path
if ([string]::IsNullOrWhiteSpace($TestRoot)) {
    $TestRoot = Join-Path (Split-Path $repositoryRoot -Parent) "测试"
}

$source = Join-Path $repositoryRoot ".codex"
if (-not (Test-Path -LiteralPath $source -PathType Container)) {
    throw "找不到源技能目录：$source"
}
if (-not (Test-Path -LiteralPath $TestRoot -PathType Container)) {
    throw "找不到测试目录：$TestRoot"
}

$resolvedTestRoot = (Resolve-Path -LiteralPath $TestRoot).Path
$target = Join-Path $resolvedTestRoot ".codex"
$resolvedSource = (Resolve-Path -LiteralPath $source).Path
if ($target -eq $resolvedSource) {
    throw "测试目录不能指向当前项目的 .codex 目录"
}

if (Test-Path -LiteralPath $target) {
    Remove-Item -LiteralPath $target -Recurse -Force
}

Copy-Item -LiteralPath $source -Destination $target -Recurse -Force
Write-Output "已同步技能目录：$resolvedSource -> $target"
