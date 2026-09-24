param(
    [Parameter(Mandatory)][string]$Executable,
    [Parameter(Mandatory)][string]$OutputDirectory,
    [switch]$BlankWebview,
    [switch]$DisableTestIme,
    [switch]$OptimizeIdleHeap,
    [ValidateSet('windows', 'subtitle-preview', 'audio-lab', 'transcription', 'comparison')][string]$Scenario = 'windows',
    [ValidateRange(1, 12)][int]$RecognitionRounds = 3,
    [ValidateRange(30, 1800)][int]$TimeoutSeconds = 300
)

$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).Path
$output = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($OutputDirectory)
[System.IO.Directory]::CreateDirectory($output) | Out-Null
$eventsPath = Join-Path $output 'events.jsonl'
if (Test-Path -LiteralPath $eventsPath) { throw '请为每轮测量使用新的输出目录。' }
$launch = [System.Diagnostics.ProcessStartInfo]::new()
$launch.FileName = $resolved
$launch.WorkingDirectory = Split-Path -Parent $resolved
$launch.UseShellExecute = $false
$launch.CreateNoWindow = $true
$launch.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
$launch.ArgumentList.Add('--autostarted')
$launch.Environment['SAYIT_ACCEPTANCE_EVENTS'] = $eventsPath
$launch.Environment['SAYIT_ACCEPTANCE_BLANK'] = if ($BlankWebview) { '1' } else { '0' }
$launch.Environment['SAYIT_ACCEPTANCE_NO_IME'] = if ($DisableTestIme) { '1' } else { '0' }
$launch.Environment['SAYIT_ACCEPTANCE_SCENARIO'] = $Scenario
$launch.Environment['SAYIT_ACCEPTANCE_RECOGNITION_ROUNDS'] = [string]$RecognitionRounds
$launch.Environment['SAYIT_ACCEPTANCE_OPTIMIZE_HEAP'] = if ($OptimizeIdleHeap) { '1' } else { '0' }
$launch.Environment.Remove('TOKIO_WORKER_THREADS') | Out-Null
$appProcess = [System.Diagnostics.Process]::Start($launch)
$watch = [System.Diagnostics.Stopwatch]::StartNew()
$samples = [System.Collections.Generic.List[object]]::new()
$known = @{}
$known[$appProcess.Id] = $appProcess.StartTime.ToUniversalTime().Ticks
$failure = $null
try {
    while (-not $appProcess.HasExited) {
        if ($watch.Elapsed.TotalSeconds -gt $TimeoutSeconds) { throw '验收进程超时。' }
        # 只读取进程树；不调用桌面输入、调试端口或工作集修剪。
        $all = @(Get-CimInstance Win32_Process | Select-Object ProcessId,ParentProcessId,CreationDate)
        $ids = [System.Collections.Generic.HashSet[int]]::new()
        $ids.Add($appProcess.Id) | Out-Null
        # 已确认的子进程即使父进程先退出，也继续跟踪直到它实际退出。
        foreach ($knownId in @($known.Keys)) {
            $survivor = Get-Process -Id $knownId -ErrorAction SilentlyContinue
            if ($null -ne $survivor -and $survivor.StartTime.ToUniversalTime().Ticks -eq $known[$knownId]) {
                $ids.Add($knownId) | Out-Null
            }
        }
        do {
            $added = $false
            foreach ($item in $all) {
                if ($ids.Contains([int]$item.ParentProcessId) -and $item.CreationDate -ge $appProcess.StartTime) {
                    if ($ids.Add([int]$item.ProcessId)) { $added = $true }
                }
            }
        } while ($added)
        $rows = @()
        foreach ($processId in $ids) {
            try {
                $process = Get-Process -Id $processId -ErrorAction Stop
                if ($process.StartTime -lt $appProcess.StartTime) { continue }
                $known[$processId] = $process.StartTime.ToUniversalTime().Ticks
                $rows += [pscustomobject]@{
                    pid = $processId; startTicks = $known[$processId]; name = $process.ProcessName
                    privateBytes = $process.PrivateMemorySize64; workingSetBytes = $process.WorkingSet64
                    cpuSeconds = $process.TotalProcessorTime.TotalSeconds
                    threads = $process.Threads.Count; handles = $process.HandleCount
                }
            } catch [Microsoft.PowerShell.Commands.ProcessCommandException] { }
        }
        $samples.Add([pscustomobject]@{
            timestampMs = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
            elapsedMs = $watch.Elapsed.TotalMilliseconds
            processes = $rows
        })
        Start-Sleep -Milliseconds 500
        $appProcess.Refresh()
    }
    $appProcess.WaitForExit()
    if ($appProcess.ExitCode -ne 0) { throw "验收退出码 $($appProcess.ExitCode)，查看 events.jsonl。" }
} catch {
    $failure = $_.Exception.Message
    # 只结束本脚本创建且仍为同一实例的进程，不根据名称终止其他应用。
    if (-not $appProcess.HasExited) { $appProcess.Kill(); $appProcess.WaitForExit() }
} finally {
    Start-Sleep -Seconds 2
    $survivors = @($known.Keys | ForEach-Object {
        $survivor = Get-Process -Id $_ -ErrorAction SilentlyContinue
        if ($null -ne $survivor -and $survivor.StartTime.ToUniversalTime().Ticks -eq $known[$_]) {
            [pscustomobject]@{ pid = $_; name = $survivor.ProcessName; privateBytes = $survivor.PrivateMemorySize64 }
        }
    })
    $report = [pscustomobject]@{
        executable = $resolved; executableSha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
        rootPid = $appProcess.Id; logicalProcessors = [Environment]::ProcessorCount
        blankWebview = [bool]$BlankWebview
        testImeDisabled = [bool]$DisableTestIme
        scenario = $Scenario
        recognitionRounds = $RecognitionRounds
        optimizeIdleHeap = [bool]$OptimizeIdleHeap
        failure = $failure; samples = $samples; survivingProcessesAfterExit = $survivors
        notes = @('私有字节是提交量，工作集求和包含共享页面重复计数。', 'CPU 差分仅比较同一 PID 和启动时间；跨进程退出的区间不能视为完整 CPU 总量。', '500ms 间隔另加进程枚举时间，短瞬时峰值可能漏采。')
    }
    $report | ConvertTo-Json -Depth 6 | Set-Content -LiteralPath (Join-Path $output 'processes.json') -Encoding utf8
}
if ($failure) { throw $failure }
$events = @(Get-Content -LiteralPath $eventsPath | ForEach-Object { $_ | ConvertFrom-Json })
if ($events[-1].stage -ne 'completed') { throw '验收未输出完成记录。' }
Write-Output "验收完成：$output，共 $($samples.Count) 个进程采样点。"
