import { useEffect, useRef } from "react";

import { isSupportedFileModel } from "@/features/asr/modelOptions";
import { DEFAULT_TRANSCRIPTION_PARAMS, type TranscriptionParams } from "@/store/useTranscriptionStore";

export const PARAMS_SAVE_DEBOUNCE_MS = 450;

/** 把供应商配置里存的识别参数收敛成完整、合法的一份。 */
export function normalizeStoredParams(value: unknown): TranscriptionParams {
  const source = value && typeof value === "object" ? (value as Record<string, unknown>) : {};
  const speakerCount = Number(source.speakerCount);
  return {
    ...DEFAULT_TRANSCRIPTION_PARAMS,
    model:
      typeof source.model === "string" && isSupportedFileModel(source.model)
        ? source.model
        : DEFAULT_TRANSCRIPTION_PARAMS.model,
    languageHints: Array.isArray(source.languageHints)
      ? source.languageHints.filter((item): item is string => typeof item === "string")
      : [],
    diarizationEnabled: !!source.diarizationEnabled,
    speakerCount: Number.isFinite(speakerCount) && speakerCount > 0 ? speakerCount : null,
  };
}

export function sameParams(a: TranscriptionParams, b: TranscriptionParams) {
  return JSON.stringify(a) === JSON.stringify(b);
}

export interface TranscriptionParamsSyncOptions {
  /** 供应商配置里存着的那份（原始值，未归一化）。对象身份变化即视为一次外部更新。 */
  stored: unknown;
  /** 界面上当前编辑中的参数。 */
  params: TranscriptionParams;
  /** 没有可用供应商时不写盘。 */
  enabled: boolean;
  replaceParams: (next: TranscriptionParams) => void;
  save: (next: TranscriptionParams) => Promise<void>;
  onMessage: (text: string) => void;
  debounceMs?: number;
}

/**
 * 把「识别参数」在界面与供应商配置之间双向同步：外部变化回填到界面，界面改动防抖写盘。
 *
 * 关键点是**识别自己写出去的回声**。保存会让 provider store 整体换一份新的 profiles，
 * `stored` 的对象身份因此每次保存都变一次。原实现在回填 effect 里无条件
 * `replaceParams(stored)`，于是保存往返期间用户做的第二次修改会被回滚；而 `params`
 * 被改回旧值又会让防抖 effect 重跑、连带 clearTimeout 掉待触发的那次保存——改动
 * 既不生效也不入库，界面却仍然显示「识别参数已保存」。
 *
 * 所以：写盘前就先把 key 记进 `lastSavedRef`（写盘过程中回填 effect 就会跑，
 * 事后再记就来不及），回填时只要认出磁盘上的值正是自己刚写的那份就整个跳过。
 */
export function useTranscriptionParamsSync({
  stored,
  params,
  enabled,
  replaceParams,
  save,
  onMessage,
  debounceMs = PARAMS_SAVE_DEBOUNCE_MS,
}: TranscriptionParamsSyncOptions) {
  const hydratedRef = useRef(false);
  const lastSavedRef = useRef("");

  useEffect(() => {
    const next = normalizeStoredParams(stored);
    const key = JSON.stringify(next);
    const echoOfOwnSave = hydratedRef.current && key === lastSavedRef.current;
    hydratedRef.current = true;
    if (echoOfOwnSave) return;
    lastSavedRef.current = key;
    if (!sameParams(params, next)) replaceParams(next);
    // params 故意不进依赖：这里只响应外部（供应商配置）的变化。
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [stored]);

  useEffect(() => {
    if (!hydratedRef.current || !enabled) return;
    const key = JSON.stringify(params);
    if (key === lastSavedRef.current) return;
    const timer = window.setTimeout(async () => {
      const previous = lastSavedRef.current;
      lastSavedRef.current = key;
      try {
        await save(params);
        onMessage("识别参数已保存。");
      } catch (error) {
        lastSavedRef.current = previous;
        onMessage(`识别参数保存失败：${String(error)}`);
      }
    }, debounceMs);
    return () => window.clearTimeout(timer);
  }, [params, enabled, save, onMessage, debounceMs]);
}
