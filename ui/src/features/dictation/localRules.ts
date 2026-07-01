// 本地处理对外入口：在 Web Worker 里跑正则管道，带超时保护。
// 超时 / worker 创建失败时回退原文（或主线程同步执行），保证听写流程不被卡住。
import { applyRulesPure, type LocalRule } from "./localRulesEngine";

export interface LocalRunResult {
  text: string;
  timedOut: boolean;
  error?: string;
}

const DEFAULT_TIMEOUT_MS = 400;

export function runLocalRules(
  text: string,
  rules: LocalRule[],
  timeoutMs = DEFAULT_TIMEOUT_MS,
): Promise<LocalRunResult> {
  const active = rules.filter((r) => r.enabled && r.pattern);
  // 没有可执行规则：直接返回，省去 worker 启动开销。
  if (active.length === 0) return Promise.resolve({ text, timedOut: false });

  return new Promise((resolve) => {
    let worker: Worker | null = null;
    let settled = false;

    const finish = (result: LocalRunResult) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (worker) worker.terminate();
      resolve(result);
    };

    // 超时即终止 worker（这是真正杀死灾难性回溯的手段），回退原文。
    const timer = setTimeout(() => finish({ text, timedOut: true }), timeoutMs);

    try {
      worker = new Worker(new URL("./localRules.worker.ts", import.meta.url), { type: "module" });
      worker.onmessage = (e: MessageEvent<{ ok: boolean; result?: string; error?: string }>) => {
        const d = e.data;
        if (d.ok) finish({ text: typeof d.result === "string" ? d.result : text, timedOut: false });
        else finish({ text, timedOut: false, error: d.error });
      };
      worker.onerror = (e) => finish({ text, timedOut: false, error: String(e.message || e) });
      worker.postMessage({ text, rules: active });
    } catch (err) {
      // worker 不可用：退回主线程同步执行（无超时保护，但保证功能可用）。
      try {
        finish({ text: applyRulesPure(text, active), timedOut: false });
      } catch (e) {
        finish({ text, timedOut: false, error: String(e) });
      }
    }
  });
}
