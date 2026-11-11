import { useEffect, useState } from "react";
import { CMD, cmd } from "@/lib/tauri";

/** 后端 `detect_hardware` 的结果；读不到的项为 0 或空。 */
export interface HardwareSummary {
  gpuName: string | null;
  gpuMemoryMb: number;
  memoryMb: number;
}

/** 本地模型对电脑的要求。数字来自实测，见 docs/experience 里本地模型的实测记录。 */
export interface LocalModelNeed {
  /** 一句话的占用说明。 */
  summary: string;
  /** 跑得顺需要的独立显存。 */
  gpuMemoryMb?: number;
  /** required：没有够用的显卡就别用；preferred：没有也能用处理器跑，只是慢。 */
  gpu?: "required" | "preferred";
}

export const LOCAL_MODEL_NEEDS: Record<string, LocalModelNeed> = {
  "official.llama-polish-4b": { summary: "占约 3.5 GB 显存，需要独立显卡", gpuMemoryMb: 6 * 1024, gpu: "required" },
  "official.transcribe-qwen3-asr-1.7b": { summary: "占约 1.8 GB 显存；没有显卡时用处理器，占约 2.5 GB 内存", gpuMemoryMb: 4 * 1024, gpu: "preferred" },
  "official.transcribe-qwen3-asr-0.6b": { summary: "占约 1 GB 显存；没有显卡时用处理器，占约 1.5 GB 内存", gpuMemoryMb: 2 * 1024, gpu: "preferred" },
  "official.sherpa-sensevoice-small": { summary: "只用处理器，占约 450 MB 内存" },
  "official.sherpa-funasr-nano-ctc": { summary: "只用处理器，占约 550 MB 内存" },
  "official.sherpa-paraformer-online": { summary: "只用处理器，占约 300 MB 内存" },
  "official.sherpa-ct-punct-zh-en": { summary: "只用处理器，占约 100 MB 内存" },
};

const gb = (mb: number) => `${Math.round(mb / 1024)} GB`;

function known(hardware: HardwareSummary | undefined): hardware is HardwareSummary {
  return Boolean(hardware && (hardware.gpuMemoryMb || hardware.memoryMb));
}

/** 这台电脑的一句话描述；什么都没读到时为空。 */
export function hardwareLabel(hardware: HardwareSummary | undefined): string {
  if (!known(hardware)) return "";
  const gpu = hardware.gpuMemoryMb >= 1024 && hardware.gpuName
    ? `${hardware.gpuName}（${gb(hardware.gpuMemoryMb)} 显存）`
    : "没有独立显卡";
  return hardware.memoryMb ? `${gpu} · ${gb(hardware.memoryMb)} 内存` : gpu;
}

/** 系统报出的显存比标称值略少（6 GB 的卡报 6000 MB 左右），比较时留出这点余量。 */
const NOMINAL_SLACK_MB = 512;

/** 显卡够不够跑一个需要这么多显存的模型；没读到硬件信息时不下结论。 */
export function gpuEnough(hardware: HardwareSummary | undefined, gpuMemoryMb: number): boolean | undefined {
  return known(hardware) ? hardware.gpuMemoryMb + NOMINAL_SLACK_MB >= gpuMemoryMb : undefined;
}

/** 某个本地模型在这台电脑上合不合适。只用处理器的模型和没读到硬件信息时不提示。 */
export function hardwareFit(
  need: LocalModelNeed | undefined,
  hardware: HardwareSummary | undefined,
): { ok: boolean; text: string } | undefined {
  if (!need?.gpu || !need.gpuMemoryMb) return undefined;
  const enough = gpuEnough(hardware, need.gpuMemoryMb);
  if (enough === undefined) return undefined;
  if (enough) return { ok: true, text: "这台电脑可以流畅运行" };
  return need.gpu === "required"
    ? { ok: false, text: `这台电脑的显卡不够，建议至少 ${gb(need.gpuMemoryMb)} 显存` }
    : { ok: false, text: "这台电脑的显存偏小，能用，但可能比较慢" };
}

/** 这台电脑的硬件信息；读到之前和读不到时为空，界面只是少一句提示。 */
export function useHardware(): HardwareSummary | undefined {
  const [hardware, setHardware] = useState<HardwareSummary>();
  useEffect(() => {
    let active = true;
    cmd<HardwareSummary>(CMD.detectHardware)
      .then((value) => active && setHardware(value))
      .catch((reason) => console.error("读取硬件信息失败", reason));
    return () => { active = false; };
  }, []);
  return hardware;
}
