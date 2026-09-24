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
  "packet-size": { type: "string", default: "4096" },
  "sample-rate": { type: "string", default: "48000" },
  "runtime-workers": { type: "string", default: "0" },
} });
if (!values.executable || !values.output) {
  throw new Error("必须指定 --executable 测试程序和 --output 结果文件");
}
const seconds = Number(values.seconds);
const runs = Number(values.runs);
const packetSize = Number(values["packet-size"]);
const sampleRate = Number(values["sample-rate"]);
const runtimeWorkers = Number(values["runtime-workers"]);
if (!Number.isInteger(runtimeWorkers) || runtimeWorkers < 0 || runtimeWorkers > 128) {
  throw new Error("runtime-workers 必须是 0～128 的整数，0 使用逻辑处理器数");
}
if (!Number.isInteger(seconds) || seconds < 1 || seconds > 1800
    || !Number.isInteger(runs) || runs < 1 || runs > 20) {
  throw new Error("seconds 必须是 1～1800 的整数，runs 必须是 1～20 的整数");
}
if (!Number.isInteger(packetSize) || packetSize < 1 || packetSize > 2_880_000
    || !Number.isInteger(sampleRate) || sampleRate < 8_000 || sampleRate > 192_000) {
  throw new Error("packet-size 必须为 1～2880000，sample-rate 必须为 8000～192000");
}
const executable = resolve(values.executable);
const testNames = {
  "comparison-delivery": "application::compare::delivery::tests::stalled_comparison_profile",
  "comparison-delivery-legacy": "application::compare::delivery::tests::stalled_comparison_profile",
  "subtitle-delivery": "application::subtitles::delivery_tests::stalled_renderer_profile",
  "subtitle-delivery-legacy": "application::subtitles::delivery_tests::stalled_renderer_profile",
  "runtime-scheduling": "runtime_performance_tests::scheduling_profile",
  "subtitle-retention": "application::subtitles::retention::tests::long_session_profile",
  "subtitle-retention-legacy": "application::subtitles::retention::tests::long_session_profile",
  "event-fanout": "application::events::performance_tests::fanout_profile",
  "event-fanout-legacy": "application::events::performance_tests::fanout_profile",
  "event-small-fanout": "application::events::performance_tests::fanout_profile",
  "event-small-fanout-legacy": "application::events::performance_tests::fanout_profile",
  "transcription-retention": "application::transcription::retention_tests::repeated_result_profile",
  "transcription-retention-legacy": "application::transcription::retention_tests::repeated_result_profile",
  "gesture-idle": "desktop::mouse_gesture::scheduling_tests::idle_monitor_profile",
  "gesture-idle-legacy": "desktop::mouse_gesture::scheduling_tests::idle_monitor_profile",
  "gesture-enabled-idle": "desktop::mouse_gesture::scheduling_tests::idle_monitor_profile",
  "gesture-enabled-idle-legacy": "desktop::mouse_gesture::scheduling_tests::idle_monitor_profile",
  "host-network-wait": "providers::plugin_runtime::control::performance_tests::network_wait_profile",
  "host-network-wait-legacy": "providers::plugin_runtime::control::performance_tests::network_wait_profile",
  "host-network-cancel": "providers::plugin_runtime::control::performance_tests::network_wait_profile",
  "host-network-cancel-legacy": "providers::plugin_runtime::control::performance_tests::network_wait_profile",
  "asr-idle-session": "commands::asr::session_wait::tests::idle_session_profile",
  "asr-idle-session-legacy": "commands::asr::session_wait::tests::idle_session_profile",
  "asr-session-latency": "commands::asr::session_wait::tests::session_notification_latency_profile",
  "asr-session-latency-legacy": "commands::asr::session_wait::tests::session_notification_latency_profile",
  "compare-startup": "application::compare::performance_tests::startup_storage_profile",
  "compare-startup-legacy": "application::compare::performance_tests::startup_storage_profile",
  "asr-spool": "asr_input::spool_performance_tests::live_queue_profile",
  "asr-spool-legacy": "asr_input::spool_performance_tests::live_queue_profile",
  "asr-short": "asr_input::spool_performance_tests::live_queue_profile",
  "asr-short-legacy": "asr_input::spool_performance_tests::live_queue_profile",
  "asr-paced": "asr_input::performance_tests::paced_backlog_profile",
  "asr-paced-legacy": "asr_input::performance_tests::paced_backlog_profile",
  "capture": "desktop::backend_mic::capture_tests::capture_memory_profile",
  "capture-legacy": "desktop::backend_mic::capture_tests::capture_memory_profile",
  "audio-lab": "application::audio_lab::performance_tests::offline_audio_memory_profile",
  decode: "audio_prep::performance_tests::file_decode_memory_profile",
  "wav-export": "application::compare::performance_tests::wav_export_memory_profile",
  "compare-recording": "application::compare::performance_tests::realtime_recording_memory_profile",
  "realtime-dsp": "audio_dsp::performance_tests::realtime_dsp_memory_profile",
  "compare-playback": "application::compare::performance_tests::uploaded_playback_memory_profile",
  "compare-playback-legacy": "application::compare::performance_tests::uploaded_playback_memory_profile",
  "asr-cancel": "asr_input::performance_tests::cancel_backlog_profile",
  "asr-cancel-legacy": "asr_input::performance_tests::cancel_backlog_profile",
  "compare-file-storage": "application::compare::performance_tests::file_recording_storage_profile",
  "compare-file-storage-legacy": "application::compare::performance_tests::file_recording_storage_profile",
  "recording-storage": "audio_wav::recording::performance_tests::recording_storage_profile",
  "recording-storage-legacy": "audio_wav::recording::performance_tests::recording_storage_profile",
};
const testName = testNames[values.scenario];
if (!testName) throw new Error(`未知场景，可选：${Object.keys(testNames).join("、")}`);
const measurements = [];
for (let run = 0; run < runs; run++) {
  const result = spawnSync(executable, [
    testName,
    "--ignored", "--exact", "--nocapture", "--test-threads=1",
  ], {
    encoding: "utf8",
    env: { ...process.env, SAYIT_PERF_AUDIO_SECONDS: String(seconds),
      SAYIT_PERF_RUNTIME_WORKERS: String(runtimeWorkers),
      SAYIT_PERF_COMPARISON_DELIVERY_LEGACY: values.scenario === "comparison-delivery-legacy" ? "1" : "0",
      SAYIT_PERF_TRANSLATION_DELIVERY_LEGACY: values.scenario === "subtitle-delivery-legacy" ? "1" : "0",
      SAYIT_PERF_SUBTITLE_LEGACY: values.scenario === "subtitle-retention-legacy" ? "1" : "0",
      SAYIT_PERF_EVENT_LEGACY: ["event-fanout-legacy", "event-small-fanout-legacy"].includes(values.scenario) ? "1" : "0",
      SAYIT_PERF_EVENT_SMALL: values.scenario.startsWith("event-small") ? "1" : "0",
      SAYIT_PERF_TRANSCRIPTION_LEGACY: values.scenario === "transcription-retention-legacy" ? "1" : "0",
      SAYIT_PERF_GESTURE_LEGACY: ["gesture-idle-legacy", "gesture-enabled-idle-legacy"].includes(values.scenario) ? "1" : "0",
      SAYIT_PERF_GESTURE_ENABLED: values.scenario.startsWith("gesture-enabled") ? "1" : "0",
      SAYIT_PERF_HOST_WAIT_LEGACY: ["host-network-wait-legacy", "host-network-cancel-legacy"].includes(values.scenario) ? "1" : "0",
      SAYIT_PERF_HOST_WAIT_CANCEL: values.scenario.startsWith("host-network-cancel") ? "1" : "0",
      SAYIT_PERF_SESSION_POLL_LEGACY: ["asr-idle-session-legacy", "asr-session-latency-legacy"].includes(values.scenario) ? "1" : "0",
      SAYIT_PERF_STARTUP_LEGACY: values.scenario === "compare-startup-legacy" ? "1" : "0",
      SAYIT_PERF_SPOOL_LEGACY: ["asr-spool-legacy", "asr-short-legacy"].includes(values.scenario) ? "1" : "0",
      SAYIT_PERF_SPOOL_SHORT: values.scenario.startsWith("asr-short") ? "1" : "0",
      SAYIT_PERF_PACED_LEGACY: values.scenario === "asr-paced-legacy" ? "1" : "0",
      SAYIT_PERF_CAPTURE_LEGACY: values.scenario === "capture-legacy" ? "1" : "0",
      SAYIT_PERF_SAMPLE_RATE: String(sampleRate), SAYIT_PERF_PACKET_SIZE: String(packetSize),
      SAYIT_PERF_PLAYBACK_LEGACY: values.scenario === "compare-playback-legacy" ? "1" : "0",
      SAYIT_PERF_CANCEL_LEGACY: values.scenario === "asr-cancel-legacy" ? "1" : "0",
      SAYIT_PERF_RECORDING_LEGACY: ["recording-storage-legacy", "compare-file-storage-legacy"].includes(values.scenario) ? "1" : "0",
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
