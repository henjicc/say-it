$repositoryRoot = Join-Path $PSScriptRoot ".."
Push-Location -LiteralPath $repositoryRoot
try {
    if (-not (Test-Path -LiteralPath ".codex" -PathType Container)) {
        throw "找不到源技能目录：.codex"
    }
    if (-not (Test-Path -LiteralPath "测试" -PathType Container)) {
        throw "找不到项目测试目录：测试"
    }

    if (Test-Path -LiteralPath "测试\.codex") {
        Remove-Item -LiteralPath "测试\.codex" -Recurse -Force
    }

    Copy-Item -LiteralPath ".codex" -Destination "测试\.codex" -Recurse -Force
    Write-Output "已同步技能目录：.codex -> 测试\.codex"
}
finally {
    Pop-Location
}
