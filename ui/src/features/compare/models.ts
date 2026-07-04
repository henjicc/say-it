import {
  FILE_ASR_MODEL_OPTIONS,
  REALTIME_ASR_MODEL_OPTIONS,
  type AsrModelOption,
} from "@/features/asr/modelOptions";

export type CompareModelKind = "realtime" | "file";

export interface CompareModelOption extends AsrModelOption {
  kind: CompareModelKind;
}

const MERGED_MODEL_OPTIONS: CompareModelOption[] = [
  ...REALTIME_ASR_MODEL_OPTIONS.map((option) => ({ ...option, kind: "realtime" as const })),
  ...FILE_ASR_MODEL_OPTIONS.map((option) => ({ ...option, kind: "file" as const })),
];

const MODEL_KIND_BY_VALUE = new Map<string, CompareModelKind>(
  MERGED_MODEL_OPTIONS.map((option) => [option.value, option.kind]),
);

export function mergedModelOptions(): CompareModelOption[] {
  return MERGED_MODEL_OPTIONS;
}

export function modelKind(value: string): CompareModelKind | null {
  return MODEL_KIND_BY_VALUE.get(value) ?? null;
}
