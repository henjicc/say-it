import {
  FILE_ASR_MODEL_OPTIONS,
  REALTIME_ASR_MODEL_OPTIONS,
  type AsrModelOption,
} from "@/features/asr/modelOptions";

export type CompareModelKind = "realtime" | "file";

export interface CompareModelOption extends AsrModelOption {
  kind: CompareModelKind;
}

export function mergedModelOptions(): CompareModelOption[] {
  return [
    ...REALTIME_ASR_MODEL_OPTIONS.map((option) => ({ ...option, kind: "realtime" as const })),
    ...FILE_ASR_MODEL_OPTIONS.map((option) => ({ ...option, kind: "file" as const })),
  ];
}

export function modelKind(value: string): CompareModelKind | null {
  return mergedModelOptions().find((option) => option.value === value)?.kind ?? null;
}
