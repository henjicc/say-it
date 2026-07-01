import { useEffect, useRef } from "react";
import { Card, CardTitle, CardDescription } from "@/components/ui/Card";
import { Button } from "@/components/ui/Button";
import { Slider } from "@/components/ui/Slider";
import { CheckField } from "@/components/ui/Field";
import { cn } from "@/lib/cn";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useAudioStore } from "@/store/useAudioStore";
import { dspDefaults } from "@/lib/audio-dsp";
import * as lab from "@/features/audio/lab";

const toneClass: Record<string, string> = {
  "": "text-white/50",
  ok: "text-[#25c36f]",
  err: "text-[#ff6b6b]",
};

const fmt = {
  targetLufs: (v: number) => `${v.toFixed(1)} LUFS`,
  maxGainDb: (v: number) => `${v.toFixed(1)} dB`,
  peakLimitDbfs: (v: number) => `${v.toFixed(1)} dB`,
  denoiseStrength: (v: number) => `${Math.round(v * 100)}%`,
  vadGate: (v: number) => (v <= 0 ? "关闭" : v.toFixed(2)),
};

export function AudioView() {
  const prefs = useDictPrefs((s) => s.prefs);
  const patch = useDictPrefs((s) => s.patch);
  const { recording, recInfo, recTone, level, canPlay, meters, labStatus, labStatusTone } =
    useAudioStore();
  const origRef = useRef<HTMLCanvasElement>(null);
  const procRef = useRef<HTMLCanvasElement>(null);

  useEffect(() => {
    lab.setCanvases(origRef.current, procRef.current);
    return () => lab.setCanvases(null, null);
  }, []);

  const onParam = (key: keyof typeof fmt, value: number) => {
    patch({ [key]: value });
    lab.paramChanged();
  };

  const reset = () => {
    patch({ ...dspDefaults });
    lab.resetParams();
  };

  return (
    <div className="flex flex-col gap-4 py-2">
      <Card>
        <CardTitle>录音试听</CardTitle>
        <CardDescription>
          录一段话 → 调参数 → A/B 试听「原始 vs 处理后」。处理算法与实际语音输入共用 Rust DSP：RNNoise
          降噪 + LUFS 响度归一化。
        </CardDescription>
        <div className="mt-4 flex flex-wrap items-center gap-2">
          <Button variant={recording ? "danger" : "primary"} onClick={lab.toggleRecord}>
            {recording ? "■ 停止录音" : "● 开始录音"}
          </Button>
          <Button disabled={!canPlay} onClick={lab.playOriginal}>
            ▶ 播放原始
          </Button>
          <Button disabled={!canPlay} onClick={lab.playProcessed}>
            ▶ 播放处理后
          </Button>
          {recInfo && <span className={cn("text-xs", toneClass[recTone])}>{recInfo}</span>}
        </div>
        <div className="mt-4 flex items-center gap-3">
          <span className="text-xs text-white/40">实时电平</span>
          <div className="h-2 flex-1 overflow-hidden rounded-full bg-white/10">
            <span
              className="block h-full rounded-full bg-white transition-[width] duration-75"
              style={{ width: `${level}%` }}
            />
          </div>
        </div>
      </Card>

      <Card>
        <div className="grid grid-cols-1 gap-2 text-xs text-white/60 sm:grid-cols-3">
          <div>原始：LUFS <b className="text-white/90">{meters.olufs}</b>｜RMS <b className="text-white/90">{meters.orms}</b> dB｜峰值 <b className="text-white/90">{meters.opeak}</b> dB</div>
          <div>处理后：LUFS <b className="text-white/90">{meters.plufs}</b>｜RMS <b className="text-white/90">{meters.prms}</b> dB｜峰值 <b className="text-white/90">{meters.ppeak}</b> dB</div>
          <div>削波样本：<b className="text-white/90">{meters.clip}</b></div>
        </div>
        <div className="mt-3 text-xs text-white/40">原始波形</div>
        <canvas ref={origRef} width={860} height={90} className="mt-1 w-full rounded-lg bg-black/30" />
        <div className="mt-3 text-xs text-white/40">处理后波形（增益 + 降噪）</div>
        <canvas ref={procRef} width={860} height={90} className="mt-1 w-full rounded-lg bg-black/30" />
      </Card>

      <Card>
        <div className="grid grid-cols-1 gap-6 sm:grid-cols-2">
          <div className="flex flex-col gap-3">
            <h3 className="text-sm font-semibold text-white/80">响度归一化</h3>
            <Slider label="目标响度" min={-30} max={-14} step={0.5} value={prefs.targetLufs} format={fmt.targetLufs} onChange={(v) => onParam("targetLufs", v)} />
            <Slider label="最大提升" min={0} max={80} step={1} value={prefs.maxGainDb} format={fmt.maxGainDb} onChange={(v) => onParam("maxGainDb", v)} />
            <Slider label="峰值上限" min={-6} max={-0.5} step={0.5} value={prefs.peakLimitDbfs} format={fmt.peakLimitDbfs} onChange={(v) => onParam("peakLimitDbfs", v)} />
            <p className="text-xs text-white/40">
              建议语音目标先用 -20 LUFS；如果希望更响可试 -18 LUFS。最大提升用于防止把近似静音的底噪硬拉上来。
            </p>
          </div>
          <div className="flex flex-col gap-3">
            <h3 className="text-sm font-semibold text-white/80">RNNoise 降噪</h3>
            <CheckField
              checked={prefs.denoiseEnabled}
              onChange={(v) => {
                patch({ denoiseEnabled: v });
                lab.paramChanged();
              }}
            >
              启用降噪
            </CheckField>
            <Slider label="降噪强度" min={0} max={1} step={0.05} value={prefs.denoiseStrength} format={fmt.denoiseStrength} onChange={(v) => onParam("denoiseStrength", v)} />
            <Slider label="VAD 静音门" min={0} max={0.9} step={0.05} value={prefs.vadGate} format={fmt.vadGate} onChange={(v) => onParam("vadGate", v)} />
            <p className="text-xs text-white/40">
              降噪强度 100% 是完整 RNNoise 输出；如果声音发闷可降到 70%~85%。VAD 静音门默认关闭，只有停顿底噪特别明显时再小幅打开。
            </p>
          </div>
        </div>
        <div className="mt-4 flex items-center gap-3">
          <Button size="sm" onClick={reset}>
            恢复默认
          </Button>
          <span className={cn("text-xs", toneClass[labStatusTone])}>{labStatus}</span>
        </div>
      </Card>
    </div>
  );
}
