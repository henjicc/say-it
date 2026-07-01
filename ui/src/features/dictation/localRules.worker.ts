// 本地处理 Web Worker：把正则管道放到独立线程跑，配合主线程超时，
// 防止用户自定义的「灾难性回溯」正则冻住主线程 / 听写流程。
import { applyRulesPure, type LocalRule } from "./localRulesEngine";

const ctx = self as unknown as Worker;

ctx.onmessage = (e: MessageEvent<{ text: string; rules: LocalRule[] }>) => {
  const { text, rules } = e.data || ({} as { text: string; rules: LocalRule[] });
  try {
    const result = applyRulesPure(String(text ?? ""), Array.isArray(rules) ? rules : []);
    ctx.postMessage({ ok: true, result });
  } catch (err) {
    ctx.postMessage({ ok: false, error: String((err as Error)?.message ?? err) });
  }
};
