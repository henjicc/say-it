param(
    [Parameter(Mandatory)][string]$Executable,
    [string[]]$AppArguments = @('--autostarted'),
    [ValidateRange(2, 60)][int]$ObserveSeconds = 8,
    [Parameter(Mandatory)][string]$OutputPath
)

$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Executable).Path
$started = [System.Diagnostics.Stopwatch]::StartNew()
$launch = [System.Diagnostics.ProcessStartInfo]::new()
$launch.FileName = $resolved
$launch.WorkingDirectory = Split-Path -Parent $resolved
$launch.UseShellExecute = $false
$launch.CreateNoWindow = $true
$launch.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Hidden
foreach ($argument in $AppArguments) { $launch.ArgumentList.Add($argument) }
$appProcess = [System.Diagnostics.Process]::Start($launch)
$samples = [System.Collections.Generic.List[object]]::new()
while ($started.Elapsed.TotalSeconds -lt $ObserveSeconds) {
    $appProcess.Refresh()
    if ($appProcess.HasExited) { throw "被测进程已退出，退出码 $($appProcess.ExitCode)；请检查单实例冲突或启动日志。" }
    $samples.Add([pscustomobject]@{
        elapsedMs = $started.Elapsed.TotalMilliseconds
        privateBytes = $appProcess.PrivateMemorySize64
        workingSetBytes = $appProcess.WorkingSet64
        cpuSeconds = $appProcess.TotalProcessorTime.TotalSeconds
        threadCount = $appProcess.Threads.Count
    })
    Start-Sleep -Milliseconds 50
}
$counters = Get-CimInstance Win32_PerfRawData_PerfProc_Process -Filter "IDProcess=$($appProcess.Id)" | Select-Object -First 1
if ($null -eq $counters) { throw '采样结束时进程已退出。' }
$appProcess.Refresh()
$webviewModules = @($appProcess.Modules | Where-Object { $_.ModuleName -match 'WebView|EmbeddedBrowser' } | Select-Object -ExpandProperty ModuleName)
$result = [pscustomobject]@{
    capturedAt = (Get-Date).ToString('o')
    executable = $resolved
    executableSha256 = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
    arguments = $AppArguments
    rootPid = $appProcess.Id
    observedMs = $started.Elapsed.TotalMilliseconds
    initialWorkerThreadsOverride = $env:TOKIO_WORKER_THREADS
    finalPrivateBytes = [long]$counters.PrivateBytes
    finalPrivateWorkingSetBytes = [long]$counters.WorkingSetPrivate
    finalWorkingSetBytes = [long]$counters.WorkingSet
    peakPrivateBytes = [long]$counters.PageFileBytesPeak
    peakWorkingSetBytes = [long]$counters.WorkingSetPeak
    cpuSeconds = $appProcess.TotalProcessorTime.TotalSeconds
    finalThreadCount = $appProcess.Threads.Count
    loadedWebviewModules = $webviewModules
    samples = $samples
    notes = @('本报告仅统计主进程；子进程总量另用测量进程内存.ps1 采样。', '这是新进程启动，未清空操作系统或浏览器磁盘缓存；观察时长不是启动延迟。', '采样完成后保留进程供界面与托盘验证。')
}
$result | ConvertTo-Json -Depth 5 | Set-Content -LiteralPath $OutputPath -Encoding utf8
$result | Select-Object rootPid,finalPrivateBytes,finalPrivateWorkingSetBytes,peakPrivateBytes,cpuSeconds,finalThreadCount,loadedWebviewModules | ConvertTo-Json -Compress
