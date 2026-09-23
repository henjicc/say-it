param(
    [int]$RootPid = 0,
    [ValidateSet('foreground', 'tray-idle', 'tray-cold', 'tray-idle-60s', 'after-dictation', 'subtitles-running')]
    [string]$Condition = 'foreground',
    [ValidateSet('json', 'csv')]
    [string]$Format = 'json',
    [ValidateRange(1, 60)]
    [int]$CpuSampleSeconds = 5,
    [string]$OutputPath = '',
    [switch]$IncludeThreadActivity
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

function Get-AppSample {
    # 同一份原始计数器同时提供进程树、内存与累计 CPU/I/O，避免固定 sleep 秒数
    # 代替真实采样间隔。PID 复用通过 ElapsedTime（进程启动时间基准）识别。
    $all = @(Get-CimInstance Win32_PerfRawData_PerfProc_Process)
    $root = $all | Where-Object IDProcess -eq $RootPid | Select-Object -First 1
    if ($null -eq $root) { throw "根进程 $RootPid 不存在或已退出。" }
    $ids = [System.Collections.Generic.HashSet[int]]::new()
    $queue = [System.Collections.Generic.Queue[int]]::new()
    $queue.Enqueue($RootPid)
    while ($queue.Count -gt 0) {
        $id = $queue.Dequeue()
        if (-not $ids.Add($id)) { continue }
        foreach ($child in $all | Where-Object { $_.CreatingProcessID -eq $id -and $_.ElapsedTime -ge $root.ElapsedTime }) {
            $queue.Enqueue([int]$child.IDProcess)
        }
    }
    $processes = @{}
    foreach ($item in $all) {
        if ($ids.Contains([int]$item.IDProcess)) { $processes[[int]$item.IDProcess] = $item }
    }
    $threads = @{}
    if ($IncludeThreadActivity) {
        $filter = ($ids | ForEach-Object { "IDProcess=$_" }) -join ' OR '
        foreach ($item in Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter $filter) {
            $threads["$($item.IDProcess):$($item.IDThread):$($item.ElapsedTime)"] = $item
        }
    }
    [pscustomobject]@{ Processes = $processes; Threads = $threads }
}

$before = Get-AppSample
Start-Sleep -Seconds $CpuSampleSeconds
$after = Get-AppSample
if ($before.Processes[$RootPid].ElapsedTime -ne $after.Processes[$RootPid].ElapsedTime) {
    throw '采样期间根进程已被替换，不能比较。'
}
$rows = @($after.Processes.Keys | Sort-Object | ForEach-Object {
    $current = $after.Processes[$_]
    $previous = $before.Processes[$_]
    $stable = $null -ne $previous -and $previous.ElapsedTime -eq $current.ElapsedTime
    $elapsedTicks = if ($stable) { [double]$current.Timestamp_Sys100NS - [double]$previous.Timestamp_Sys100NS } else { 0 }
    $sampled = $stable -and $elapsedTicks -gt 0
    [pscustomobject]@{
        pid = $_; name = $current.Name
        workingSetBytes = [long]$current.WorkingSet
        privateWorkingSetBytes = [long]$current.WorkingSetPrivate
        shareableWorkingSetBytes = [long]$current.WorkingSet - [long]$current.WorkingSetPrivate
        privateBytes = [long]$current.PrivateBytes
        peakPrivateBytes = [long]$current.PageFileBytesPeak
        peakWorkingSetBytes = [long]$current.WorkingSetPeak
        threadCount = [int]$current.ThreadCount
        handleCount = [int]$current.HandleCount
        sampleSeconds = if ($sampled) { $elapsedTicks / 1e7 } else { $null }
        cpuPercent = if ($sampled) { [math]::Round(([double]$current.PercentProcessorTime - [double]$previous.PercentProcessorTime) / $elapsedTicks / [Environment]::ProcessorCount * 100, 4) } else { $null }
        readBytes = if ($sampled) { [long]$current.IOReadBytesPersec - [long]$previous.IOReadBytesPersec } else { $null }
        writeBytes = if ($sampled) { [long]$current.IOWriteBytesPersec - [long]$previous.IOWriteBytesPersec } else { $null }
        otherIoBytes = if ($sampled) { [long]$current.IOOtherBytesPersec - [long]$previous.IOOtherBytesPersec } else { $null }
    }
})
$changedIds = @($before.Processes.Keys | Where-Object { -not $after.Processes.ContainsKey($_) -or $before.Processes[$_].ElapsedTime -ne $after.Processes[$_].ElapsedTime })
$newIds = @($after.Processes.Keys | Where-Object { -not $before.Processes.ContainsKey($_) -or $before.Processes[$_].ElapsedTime -ne $after.Processes[$_].ElapsedTime })
$treeStable = $changedIds.Count -eq 0 -and $newIds.Count -eq 0
$cpuComplete = $treeStable -and @($rows | Where-Object { $null -eq $_.cpuPercent }).Count -eq 0
$contextSwitches = if ($IncludeThreadActivity) {
    $sum = 0L
    foreach ($key in $after.Threads.Keys) {
        if ($before.Threads.ContainsKey($key)) {
            $sum += [long]$after.Threads[$key].ContextSwitchesPersec - [long]$before.Threads[$key].ContextSwitchesPersec
        }
    }
    $sum
} else { $null }
$result = [pscustomobject]@{
    capturedAt = (Get-Date).ToString('o'); condition = $Condition; rootPid = $RootPid
    processCount = $rows.Count
    totalWorkingSetBytes = ($rows | Measure-Object workingSetBytes -Sum).Sum
    totalPrivateWorkingSetBytes = ($rows | Measure-Object privateWorkingSetBytes -Sum).Sum
    totalPrivateBytes = ($rows | Measure-Object privateBytes -Sum).Sum
    cpuSampleSeconds = $CpuSampleSeconds
    totalCpuPercent = if ($cpuComplete) { [math]::Round(($rows | Measure-Object cpuPercent -Sum).Sum, 4) } else { $null }
    processTreeStable = $treeStable
    exitedOrReplacedPids = $changedIds
    newPids = $newIds
    contextSwitchesOfSurvivingThreads = $contextSwitches
    notes = @('工作集求和包含重复的可共享页面，不代表去重后的物理内存。', 'I/O 包括文件、网络和设备；线程切换不等于唤醒次数。', 'CPU/I/O 只比较两个采样点均存在的同一进程；瞬时峰值应另用连续跟踪测量。')
    processes = $rows
}
$rendered = if ($Format -eq 'csv') { $rows | ConvertTo-Csv -NoTypeInformation } else { $result | ConvertTo-Json -Depth 4 }
if ($OutputPath) { $rendered | Set-Content -LiteralPath $OutputPath -Encoding utf8 } else { $rendered }
