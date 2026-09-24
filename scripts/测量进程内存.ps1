param(
    [int]$RootPid = 0,
    [ValidateSet('foreground', 'tray-idle', 'tray-cold', 'tray-idle-60s', 'after-dictation', 'subtitles-running')]
    [string]$Condition = 'foreground',
    [ValidateSet('json', 'csv')]
    [string]$Format = 'json',
    [ValidateRange(1, 60)]
    [int]$CpuSampleSeconds = 5,
    [string]$OutputPath = '',
    [switch]$IncludeThreadActivity,
    [switch]$IncludeGpuActivity
)

$ErrorActionPreference = 'Stop'
if ($IncludeThreadActivity -and -not ('SayItThreadProbe' -as [type])) {
    # 只申请查询权限；不暂停线程、改变优先级或操作窗口。
    # https://learn.microsoft.com/windows/win32/api/processthreadsapi/nf-processthreadsapi-getthreaddescription
    # https://learn.microsoft.com/windows/win32/api/realtimeapiset/nf-realtimeapiset-querythreadcycletime
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public sealed class SayItThreadSample {
    public string Description;
    public ulong? CpuCycles;
    public long MonotonicTicks;
}
public static class SayItThreadProbe {
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr OpenThread(uint access, bool inherit, uint id);
    [DllImport("kernel32.dll")] static extern int GetThreadDescription(IntPtr handle, out IntPtr text);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryThreadCycleTime(IntPtr handle, out ulong cycles);
    [DllImport("kernel32.dll")] static extern IntPtr LocalFree(IntPtr memory);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr handle);
    public static SayItThreadSample Read(uint id) {
        var result = new SayItThreadSample();
        var handle = OpenThread(0x0800, false, id);
        if (handle == IntPtr.Zero) return result;
        try {
            IntPtr text;
            int status = GetThreadDescription(handle, out text);
            try { if (status >= 0) result.Description = Marshal.PtrToStringUni(text); }
            finally { if (text != IntPtr.Zero) LocalFree(text); }
            ulong cycles;
            if (QueryThreadCycleTime(handle, out cycles)) result.CpuCycles = cycles;
            result.MonotonicTicks = System.Diagnostics.Stopwatch.GetTimestamp();
        } finally { CloseHandle(handle); }
        return result;
    }
}
'@
}
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
        $parent = $all | Where-Object IDProcess -eq $id | Select-Object -First 1
        foreach ($child in $all | Where-Object { $_.CreatingProcessID -eq $id -and $_.ElapsedTime -ge $parent.ElapsedTime }) {
            $queue.Enqueue([int]$child.IDProcess)
        }
    }
    $processes = @{}
    foreach ($item in $all) {
        if ($ids.Contains([int]$item.IDProcess)) { $processes[[int]$item.IDProcess] = $item }
    }
    $threads = @{}
    $threadMetadata = @{}
    if ($IncludeThreadActivity) {
        $filter = ($ids | ForEach-Object { "IDProcess=$_" }) -join ' OR '
        foreach ($item in Get-CimInstance Win32_PerfRawData_PerfProc_Thread -Filter $filter) {
            $key = "$($item.IDProcess):$($item.IDThread):$($item.ElapsedTime)"
            $threads[$key] = $item
            $threadMetadata[$key] = [SayItThreadProbe]::Read([uint32]$item.IDThread)
        }
    }
    $gpu = $null
    if ($IncludeGpuActivity) {
        try {
            $engines = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUEngine | Where-Object {
                $_.Name -match '^pid_(\d+)_' -and $ids.Contains([int]$Matches[1])
            } | ForEach-Object { [pscustomobject]@{ instance = $_.Name; utilizationPercent = $_.UtilizationPercentage } })
            $gpuMemory = @(Get-CimInstance Win32_PerfFormattedData_GPUPerformanceCounters_GPUProcessMemory | Where-Object {
                $_.Name -match '^pid_(\d+)_' -and $ids.Contains([int]$Matches[1])
            } | ForEach-Object { [pscustomobject]@{ instance = $_.Name; dedicatedBytes = $_.DedicatedUsage; sharedBytes = $_.SharedUsage; committedBytes = $_.TotalCommitted } })
            $gpu = [pscustomobject]@{ available = $true; capturedAt = (Get-Date).ToString('o'); engines = $engines; memory = $gpuMemory }
        } catch {
            $gpu = [pscustomobject]@{ available = $false; error = $_.Exception.Message }
        }
    }
    [pscustomobject]@{ Processes = $processes; Threads = $threads; ThreadMetadata = $threadMetadata; Gpu = $gpu }
}

$rootExecutable = (Get-Process -Id $RootPid).Path
$rootExecutableHash = (Get-FileHash -LiteralPath $rootExecutable -Algorithm SHA256).Hash
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
$threadRows = @($after.Threads.Keys | Sort-Object | ForEach-Object {
    $current = $after.Threads[$_]
    $previous = $before.Threads[$_]
    $metadata = $after.ThreadMetadata[$_]
    $oldMetadata = $before.ThreadMetadata[$_]
    $elapsedTicks = if ($null -ne $previous) { [double]$current.Timestamp_Sys100NS - [double]$previous.Timestamp_Sys100NS } else { 0 }
    [pscustomobject]@{
        pid = [int]$current.IDProcess; tid = [int]$current.IDThread; description = $metadata.Description
        sampleSeconds = if ($elapsedTicks -gt 0) { $elapsedTicks / 1e7 } else { $null }
        cpuMilliseconds = if ($elapsedTicks -gt 0) { ([double]$current.PercentProcessorTime - [double]$previous.PercentProcessorTime) / 1e4 } else { $null }
        contextSwitches = if ($null -ne $previous) { [long]$current.ContextSwitchesPersec - [long]$previous.ContextSwitchesPersec } else { $null }
        cpuCycles = if ($null -ne $metadata.CpuCycles -and $null -ne $oldMetadata.CpuCycles -and $metadata.CpuCycles -ge $oldMetadata.CpuCycles) { $metadata.CpuCycles - $oldMetadata.CpuCycles } else { $null }
        cycleSampleSeconds = if ($null -ne $metadata.CpuCycles -and $null -ne $oldMetadata.CpuCycles) { ($metadata.MonotonicTicks - $oldMetadata.MonotonicTicks) / [System.Diagnostics.Stopwatch]::Frequency } else { $null }
        state = [int]$current.ThreadState; waitReason = [int]$current.ThreadWaitReason
    }
})
$result = [pscustomobject]@{
    capturedAt = (Get-Date).ToString('o'); condition = $Condition; rootPid = $RootPid
    executable = $rootExecutable; executableSha256 = $rootExecutableHash
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
    notes = @('工作集求和包含重复的可共享页面，不代表去重后的物理内存。', 'I/O 包括文件、网络和设备；线程切换不等于唤醒次数。', 'CPU/I/O 只比较两个采样点均存在的同一进程；瞬时峰值应另用连续跟踪测量。', '线程 CPU 周期不得转换为耗时；线程名称可变，查询失败为 null。GPU 是采样端点的引擎计数器，不是全区间峰值；不同引擎利用率不直接相加。')
    processes = $rows
    threads = $threadRows
    gpuBefore = $before.Gpu
    gpuAfter = $after.Gpu
}
$rendered = if ($Format -eq 'csv') { $rows | ConvertTo-Csv -NoTypeInformation } else { $result | ConvertTo-Json -Depth 4 }
if ($OutputPath) { $rendered | Set-Content -LiteralPath $OutputPath -Encoding utf8 } else { $rendered }
