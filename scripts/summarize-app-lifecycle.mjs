import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const directory = resolve(process.argv[2] ?? "");
if (!process.argv[2]) throw new Error("请指定验收输出目录");
const report = JSON.parse(readFileSync(join(directory, "processes.json"), "utf8").replace(/^\uFEFF/, ""));
const events = readFileSync(join(directory, "events.jsonl"), "utf8").trim().split(/\r?\n/).map(JSON.parse);
if (report.failure || events.at(-1)?.stage !== "completed") throw new Error("验收未完成");
const reclaim = events.at(-1)?.automaticHeapReclaim;
if (reclaim && report.autoReclaimDisabled === false && reclaim.completed < 2) throw new Error("未观察到多次任务后的自动回收");
if (reclaim && report.autoReclaimDisabled === true && reclaim.completed !== 0) throw new Error("回收对照开关未隔离自动回收");
const sum = (rows, key) => rows.reduce((total, row) => total + row[key], 0);
const scenario = report.scenario ?? "windows";
const recognitionRounds = report.recognitionRounds ?? 3;
const expected = {
  windows: { open: 10, closed: 10 },
  "subtitle-preview": { "preview-active": 5, "preview-stopped": 5 },
  "audio-lab": { "audio-recorded": 3, "audio-processed": 3, "audio-replaced": 3 },
  transcription: { "transcription-settled": recognitionRounds * 4, "transcription-idle": recognitionRounds },
  comparison: { "comparison-settled": recognitionRounds * 4, "comparison-idle": recognitionRounds, "comparison-mixed-settled": 1 },
  dictation: { "dictation-settled": recognitionRounds * 4, "dictation-idle": recognitionRounds, "dictation-file-settled": 4 },
  subtitles: { "subtitles-settled": recognitionRounds * 3, "subtitles-idle": recognitionRounds },
}[scenario];
if (!expected) throw new Error(`未知场景：${scenario}`);
const rows = events.flatMap((event, index) => {
  if (!["initial-idle", "final-idle", ...Object.keys(expected)].includes(event.stage)) return [];
  const end = events[index + 1]?.timestampMs ?? Infinity;
  const samples = report.samples.filter(sample => sample.timestampMs >= event.timestampMs && sample.timestampMs < end);
  if (samples.length < 2) throw new Error(`${event.stage}/${event.cycle} 缺少采样点`);
  // 各阶段尾部的实际采样值，不把打开过程峰值当成稳定值。
  const last = samples.at(-1);
  const previous = samples.at(-2);
  const oldProcesses = new Map(previous.processes.map(p => [`${p.pid}:${p.startTicks}`, p]));
  const stable = last.processes.length === oldProcesses.size
    && last.processes.every(p => oldProcesses.has(`${p.pid}:${p.startTicks}`));
  const cpuSeconds = stable ? last.processes.reduce((total, p) =>
    total + p.cpuSeconds - oldProcesses.get(`${p.pid}:${p.startTicks}`).cpuSeconds, 0) : null;
  return [{
    stage: event.stage, cycle: event.cycle, latencyMs: event.elapsedMs,
    processCount: last.processes.length,
    privateMiB: sum(last.processes, "privateBytes") / 1024 ** 2,
    rootPrivateMiB: (last.processes.find(p => p.pid === report.rootPid)?.privateBytes ?? NaN) / 1024 ** 2,
    threads: sum(last.processes, "threads"), handles: sum(last.processes, "handles"),
    cpuPercent: cpuSeconds === null ? null : cpuSeconds / ((last.timestampMs - previous.timestampMs) / 1000)
      / report.logicalProcessors * 100,
    cpuSampleMs: last.timestampMs - previous.timestampMs,
  }];
});
for (const [stage, count] of Object.entries(expected)) {
  if (rows.filter(row => row.stage === stage).length !== count) throw new Error(`${stage} 未完成全部 ${count} 轮`);
}
// 保留运行阶段的观测峰值；约一秒采样不能代替瞬时峰值或物理内存总量。
const phasePeaks = events.flatMap((event, index) => {
  const end = events[index + 1]?.timestampMs ?? Infinity;
  const samples = report.samples.filter(sample => sample.timestampMs >= event.timestampMs && sample.timestampMs < end);
  if (!samples.length) return [];
  let cpuSeconds = 0;
  let cpuCoveredMs = 0;
  for (let i = 1; i < samples.length; i++) {
    const previous = samples[i - 1];
    const current = samples[i];
    const old = new Map(previous.processes.map(p => [`${p.pid}:${p.startTicks}`, p]));
    if (current.processes.length !== old.size || current.processes.some(p => !old.has(`${p.pid}:${p.startTicks}`))) continue;
    cpuSeconds += current.processes.reduce((total, p) => total + p.cpuSeconds - old.get(`${p.pid}:${p.startTicks}`).cpuSeconds, 0);
    cpuCoveredMs += current.timestampMs - previous.timestampMs;
  }
  return [{
    stage: event.stage, cycle: event.cycle, samples: samples.length,
    maxPrivateMiB: Math.max(...samples.map(sample => sum(sample.processes, "privateBytes"))) / 1024 ** 2,
    maxRootPrivateMiB: Math.max(...samples.map(sample => sample.processes.find(p => p.pid === report.rootPid)?.privateBytes ?? 0)) / 1024 ** 2,
    meanCpuPercent: cpuCoveredMs ? cpuSeconds / (cpuCoveredMs / 1000) / report.logicalProcessors * 100 : null,
    cpuCoveredMs,
  }];
});
writeFileSync(join(directory, "summary.json"), JSON.stringify({ scenario, optimizeIdleHeap: report.optimizeIdleHeap ?? false, autoReclaimDisabled: report.autoReclaimDisabled ?? null, executableSha256: report.executableSha256, rows, phasePeaks }, null, 2) + "\n");
console.table(rows.map(row => ({ ...row, privateMiB: +row.privateMiB.toFixed(2), rootPrivateMiB: +row.rootPrivateMiB.toFixed(2) })));
