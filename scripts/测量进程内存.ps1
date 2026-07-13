param(
    [int]$RootPid = 0,
    [ValidateSet('foreground', 'tray-idle-60s', 'after-dictation', 'subtitles-running')]
    [string]$Condition = 'foreground',
    [ValidateSet('json', 'csv')]
    [string]$Format = 'json',
    [ValidateRange(1, 60)]
    [int]$CpuSampleSeconds = 5,
    [string]$OutputPath = ''
)

$ErrorActionPreference = 'Stop'
if ($Condition -eq 'tray-idle-60s') { Start-Sleep -Seconds 60 }
if ($RootPid -eq 0) {
    $roots = @(Get-Process -Name 'SayIt' -ErrorAction SilentlyContinue | Sort-Object StartTime)
    if ($roots.Count -ne 1) {
        throw "请用 -RootPid 指定 SayIt.exe 根进程；当前找到 $($roots.Count) 个候选进程。"
    }
    $RootPid = $roots[0].Id
}

$all = @(Get-CimInstance Win32_Process)
$ids = [System.Collections.Generic.HashSet[int]]::new()
$queue = [System.Collections.Generic.Queue[int]]::new()
$queue.Enqueue($RootPid)
while ($queue.Count -gt 0) {
    $id = $queue.Dequeue()
    if (-not $ids.Add($id)) { continue }
    foreach ($child in $all | Where-Object ParentProcessId -eq $id) { $queue.Enqueue([int]$child.ProcessId) }
}

$beforeCpu = @{}
foreach ($id in $ids) {
    $process = Get-Process -Id $id -ErrorAction SilentlyContinue
    if ($null -ne $process) { $beforeCpu[$id] = if ($null -eq $process.CPU) { 0 } else { $process.CPU } }
}
Start-Sleep -Seconds $CpuSampleSeconds
$processes = @($ids | ForEach-Object { Get-Process -Id $_ -ErrorAction SilentlyContinue })
if ($processes.Count -eq 0) { throw "根进程 $RootPid 不存在或已退出。" }
$rows = @($processes | Sort-Object Id | ForEach-Object {
    [pscustomobject]@{
        pid = $_.Id; name = $_.ProcessName; workingSetBytes = $_.WorkingSet64
        privateBytes = $_.PrivateMemorySize64
        cpuPercent = [math]::Round((($_.CPU - $beforeCpu[$_.Id]) / $CpuSampleSeconds / [Environment]::ProcessorCount) * 100, 3)
    }
})
$result = [pscustomobject]@{
    capturedAt = (Get-Date).ToString('o'); condition = $Condition; rootPid = $RootPid
    processCount = $rows.Count
    totalWorkingSetBytes = ($rows | Measure-Object workingSetBytes -Sum).Sum
    totalPrivateBytes = ($rows | Measure-Object privateBytes -Sum).Sum
    cpuSampleSeconds = $CpuSampleSeconds
    totalCpuPercent = [math]::Round(($rows | Measure-Object cpuPercent -Sum).Sum, 3)
    processes = $rows
}
$rendered = if ($Format -eq 'csv') { $rows | ConvertTo-Csv -NoTypeInformation } else { $result | ConvertTo-Json -Depth 4 }
if ($OutputPath) { $rendered | Set-Content -LiteralPath $OutputPath -Encoding utf8 } else { $rendered }
