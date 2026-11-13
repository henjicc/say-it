import { useEffect } from "react";
import { useCachedQuery, type Query } from "@/hooks/useCachedQuery";
import { isMacOS } from "@/lib/platform";
import { CMD, cmd } from "@/lib/tauri";

/** 后端 `detect_hardware` 的结果；读不到的项为 0 或空。 */
export interface HardwareSummary {
  gpuName: string | null;
  /** 显卡可用的内存；统一内存的 Mac 上是系统建议给显卡常驻的上限。 */
  gpuMemoryMb: number;
  memoryMb: number;
  /** 显卡与处理器共用内存（Apple 芯片）。 */
  unifiedMemory: boolean;
}

/** 本地模型的说明与对电脑的要求。数字来自实测，见 docs/experience 里本地模型的实测记录。 */
export interface LocalModelNeed {
  /** 一句短话说明这个模型做什么，以句号结尾。 */
  description: string;
  /** 一句话的占用说明。 */
  summary: string;
  /** 跑得顺需要的独立显存。 */
  gpuMemoryMb?: number;
  /** required：没有够用的显卡就别用；preferred：没有也能用处理器跑，只是慢。 */
  gpu?: "required" | "preferred";
}

// Mac 上这些模型都用芯片自带的显卡跑，占的是共用的内存。
export const LOCAL_MODEL_NEEDS: Record<string, LocalModelNeed> = {
  "official.transcribe-qwen3-asr-1.7b": {
    description: "最准的本地识别，自带标点，支持热词和上下文；没有逐字时间，不能生成字幕。",
    summary: isMacOS ? "占约 1.8 GB 内存，用显卡运行" : "占约 1.8 GB 显存；没有显卡时用处理器，占约 2.5 GB 内存",
    gpuMemoryMb: 4 * 1024,
    gpu: "preferred",
  },
  "official.llama-polish-4b": {
    description: "把口语整理成通顺的文字，去掉口头禅和口误。",
    summary: isMacOS ? "占约 3.5 GB 内存，用显卡运行" : "占约 3.5 GB 显存，需要独立显卡",
    gpuMemoryMb: 6 * 1024,
    gpu: "required",
  },
  "official.transcribe-qwen3-asr-0.6b": {
    description: "1.7B 的轻量版，稍不准，显存小也能跑；同样不能生成字幕。",
    summary: isMacOS ? "占约 1 GB 内存，用显卡运行" : "占约 1 GB 显存；没有显卡时用处理器，占约 1.5 GB 内存",
    gpuMemoryMb: 2 * 1024,
    gpu: "preferred",
  },
  "official.sherpa-funasr-nano-ctc": {
    description: "没有显卡时的首选，速度快，带逐字时间、能生成字幕，不带标点。",
    summary: "只用处理器，占约 550 MB 内存",
  },
  "official.sherpa-sensevoice-small": {
    description: "多语种识别，还能听粤语、日语和韩语，带逐字时间、能生成字幕，不带标点。",
    summary: "只用处理器，占约 450 MB 内存",
  },
  "official.sherpa-paraformer-online": {
    description: "边说边出字，适合实时字幕，准确率较低。",
    summary: "只用处理器，占约 300 MB 内存",
  },
  "official.sherpa-ct-punct-zh-en": {
    description: "给不带标点的识别结果自动补标点。",
    summary: "只用处理器，占约 100 MB 内存",
  },
};

/** 模型页上那一行的说明；不认识的模型为空。占用放在名称的悬停提示里。 */
export function localModelDescription(id: string): string | undefined {
  return LOCAL_MODEL_NEEDS[id]?.description;
}

/** 「本地模型」页的显示顺序，按推荐程度手动排。 */
const LOCAL_MODEL_ORDER = [
  "official.transcribe-qwen3-asr-1.7b",
  "official.llama-polish-4b",
  "official.transcribe-qwen3-asr-0.6b",
  "official.sherpa-funasr-nano-ctc",
  "official.sherpa-sensevoice-small",
  "official.sherpa-paraformer-online",
  "official.sherpa-ct-punct-zh-en",
];

/** 推荐顺序里的位置；不认识的模型排在最后。 */
export function localModelRank(id: string): number {
  const index = LOCAL_MODEL_ORDER.indexOf(id);
  return index < 0 ? LOCAL_MODEL_ORDER.length : index;
}

const gb = (mb: number) => `${Math.round(mb / 1024)} GB`;

function known(hardware: HardwareSummary | undefined): hardware is HardwareSummary {
  return Boolean(hardware && (hardware.gpuMemoryMb || hardware.memoryMb));
}

/** 这台电脑的一句话描述；什么都没读到时为空。 */
export function hardwareLabel(hardware: HardwareSummary | undefined): string {
  if (!known(hardware)) return "";
  if (hardware.unifiedMemory) {
    const chip = hardware.gpuName ?? "Apple 芯片";
    return `${chip} · ${gb(hardware.memoryMb)} 内存，显卡最多可用 ${gb(hardware.gpuMemoryMb)}`;
  }
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
  if (hardware?.unifiedMemory) {
    return need.gpu === "required"
      ? { ok: false, text: `这台 Mac 的内存偏小，显卡最多可用 ${gb(hardware.gpuMemoryMb)}，建议至少 ${gb(need.gpuMemoryMb)}` }
      : { ok: false, text: "这台 Mac 的内存偏小，能用，但可能比较慢" };
  }
  return need.gpu === "required"
    ? { ok: false, text: `这台电脑的显卡不够，建议至少 ${gb(need.gpuMemoryMb)} 显存` }
    : { ok: false, text: "这台电脑的显存偏小，能用，但可能比较慢" };
}

const GPU_ASR_PACK_ID = "official.transcribe-qwen3-asr-1.7b";
const CPU_ASR_PACK_ID = "official.sherpa-funasr-nano-ctc";

/** 这台电脑首选的本地识别模型包：显卡够用选最准的，否则选只靠处理器的；没读到硬件信息时不推荐。 */
export function recommendedAsrPack(hardware: HardwareSummary | undefined): string | undefined {
  const enough = gpuEnough(hardware, LOCAL_MODEL_NEEDS[GPU_ASR_PACK_ID].gpuMemoryMb!);
  if (enough === undefined) return undefined;
  return enough ? GPU_ASR_PACK_ID : CPU_ASR_PACK_ID;
}

export const hardwareQuery: Query<HardwareSummary> = {
  key: "hardware",
  load: () => cmd<HardwareSummary>(CMD.detectHardware),
};

/** 这台电脑的硬件信息；读到之前和读不到时为空，界面只是少一句提示。 */
export function useHardware(): HardwareSummary | undefined {
  const { data, error } = useCachedQuery(hardwareQuery);
  useEffect(() => {
    if (error) console.error("读取硬件信息失败", error);
  }, [error]);
  return data;
}
