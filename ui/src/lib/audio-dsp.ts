// 与 Rust audio_dsp.rs 对应的共享 DSP 参数形状与浏览器侧编码 helper。
// 实际处理在 Rust（nnnoiseless + ebur128）；本文件让 UI / 实时采集 / 调试台用同一参数结构。
// 移植自旧 ui/audio-dsp.js。

export interface DspParams {
  denoiseEnabled: boolean;
  denoiseStrength: number;
  targetLufs: number;
  peakLimitDbfs: number;
  maxGainDb: number;
  vadGate: number;
}

export const dspDefaults: DspParams = {
  denoiseEnabled: true,
  denoiseStrength: 1.0,
  targetLufs: -20.0,
  peakLimitDbfs: -1.0,
  maxGainDb: 40.0,
  vadGate: 0.0,
};

export function dspParamsFromPrefs(prefs: Partial<DspParams> = {}): DspParams {
  return {
    denoiseEnabled: !!prefs.denoiseEnabled,
    denoiseStrength: Number(prefs.denoiseStrength ?? dspDefaults.denoiseStrength),
    targetLufs: Number(prefs.targetLufs ?? dspDefaults.targetLufs),
    peakLimitDbfs: Number(prefs.peakLimitDbfs ?? dspDefaults.peakLimitDbfs),
    maxGainDb: Number(prefs.maxGainDb ?? dspDefaults.maxGainDb),
    vadGate: Number(prefs.vadGate ?? dspDefaults.vadGate),
  };
}

export function float32ToBase64(samples: Float32Array): string {
  const bytes = new Uint8Array(samples.length * 4);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < samples.length; i += 1) {
    view.setFloat32(i * 4, samples[i], true);
  }
  let binary = "";
  const chunkSize = 0x8000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunkSize));
  }
  return btoa(binary);
}

export function base64ToFloat32(base64: string): Float32Array {
  const binary = atob(base64);
  const length = Math.floor(binary.length / 4);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  const view = new DataView(bytes.buffer);
  const samples = new Float32Array(length);
  for (let i = 0; i < length; i += 1) {
    samples[i] = view.getFloat32(i * 4, true);
  }
  return samples;
}

export function measure(samples: Float32Array): { rms: number; peak: number } {
  let sum = 0;
  let peak = 0;
  for (let i = 0; i < samples.length; i += 1) {
    const a = Math.abs(samples[i]);
    sum += samples[i] * samples[i];
    if (a > peak) peak = a;
  }
  return {
    rms: Math.sqrt(sum / Math.max(1, samples.length)),
    peak,
  };
}
