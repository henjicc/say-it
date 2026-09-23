import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { parseArgs } from "node:util";

// 测试二进制由 run-rust-tests.mjs --release --no-run 构建。
// 每次采样创建独立进程，Windows 的进程历史峰值才有可比性。
const { values } = parseArgs({ options: {
  executable: { type: "string" },
  output: { type: "string" },
  seconds: { type: "string", default: "300" },
  runs: { type: "string", default: "3" },
  denoise: { type: "boolean", default: false },
  scenario: { type: "string", default: "audio-lab" },
} });
if (!values.executable || !values.output) {
  throw new Error("必须指定 --executable 测试程序和 --output 结果文件");
}
const seconds = Number(values.seconds);
const runs = Number(values.runs);
if (!Number.isInteger(seconds) || seconds < 1 || seconds > 1800
    || !Number.isInteger(runs) || runs < 1 || runs > 20) {
  throw new Error("seconds 必须是 1～1800 的整数，runs 必须是 1～20 的整数");
}
const executable = resolve(values.executable);
const testNames = {
  "audio-lab": "application::audio_lab::performance_tests::offline_audio_memory_profile",
  decode: "audio_prep::performance_tests::file_decode_memory_profile",
};
const testName = testNames[values.scenario];
if (!testName) throw new Error("scenario 必须是 audio-lab 或 decode");
const measurements = [];
for (let run = 0; run < runs; run++) {
  const result = spawnSync(executable, [
    testName,
    "--ignored", "--exact", "--nocapture", "--test-threads=1",
  ], {
    encoding: "utf8",
    env: { ...process.env, SAYIT_PERF_AUDIO_SECONDS: String(seconds),
      SAYIT_PERF_DENOISE: values.denoise ? "1" : "0" },
  });
  if (result.error) throw result.error;
  if (result.status !== 0) {
    throw new Error(`音频基准失败（${result.status}）：${result.stderr}\n${result.stdout}`);
  }
  const match = result.stdout.match(/PERF_RESULT (\{[^\r\n]+\})/);
  if (!match) throw new Error(`测试未输出基准结果：${result.stdout}`);
  measurements.push(JSON.parse(match[1]));
}
const report = {
  capturedAt: new Date().toISOString(),
  platform: process.platform,
  arch: process.arch,
  executable,
  executableSha256: createHash("sha256").update(readFileSync(executable)).digest("hex"),
  measurements,
};
writeFileSync(values.output, `${JSON.stringify(report, null, 2)}\n`, "utf8");
console.table(measurements.map(({ elapsedMs, peakPrivateBytes, outputHash }) => ({
  elapsedMs, peakPrivateMiB: peakPrivateBytes / 1024 ** 2, outputHash,
})));
