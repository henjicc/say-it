import { readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";

const directory = resolve(process.argv[2] ?? "");
if (!process.argv[2]) throw new Error("请指定验收输出目录");
const report = JSON.parse(readFileSync(join(directory, "processes.json"), "utf8").replace(/^\uFEFF/, ""));
const events = readFileSync(join(directory, "events.jsonl"), "utf8").trim().split(/\r?\n/).map(JSON.parse);
if (report.failure || events.at(-1)?.stage !== "completed") throw new Error("验收未完成");
const sum = (rows, key) => rows.reduce((total, row) => total + row[key], 0);
const rows = events.flatMap((event, index) => {
  if (!["initial-idle", "open", "closed", "final-idle"].includes(event.stage)) return [];
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
if (rows.filter(row => row.stage === "open").length !== 10 || rows.filter(row => row.stage === "closed").length !== 10) {
  throw new Error("未完成全部 10 轮窗口开关");
}
writeFileSync(join(directory, "summary.json"), JSON.stringify({ executableSha256: report.executableSha256, rows }, null, 2) + "\n");
console.table(rows.map(row => ({ ...row, privateMiB: +row.privateMiB.toFixed(2), rootPrivateMiB: +row.rootPrivateMiB.toFixed(2) })));
