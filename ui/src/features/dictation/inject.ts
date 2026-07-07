import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useDictPrefs } from "@/store/useDictPrefs";
import { useDictationStore } from "@/store/useDictationStore";
import { playCue } from "@/lib/cues";
import { runLocalRules } from "./localRules";
import { getInjectMethod } from "./hotkeys";
import { resetIndicatorPreview } from "./indicatorBridge";
import { dictSession, pushDictLog, setDictationStatus } from "./session";

async function runLocalProcessing(text: string): Promise<string> {
  const prefs = useDictPrefs.getState().prefs;
  if (!prefs.localRulesEnabled) return text;
  const active = prefs.localRules.filter((r) => r.enabled && r.pattern).length;
  if (active === 0) return text;
  setDictationStatus("识别完成，正在本地处理…");
  const result = await runLocalRules(text, prefs.localRules);
  const out = result.text.trim() || text;
  if (result.timedOut) {
    pushDictLog("本地处理超时，已回退原文。");
  } else if (result.error) {
    pushDictLog(`本地处理出错，已回退原文：${result.error}`);
  } else {
    pushDictLog(`本地处理：${text.length} → ${out.length} 字（启用规则 ${active} 条）`);
  }
  return out;
}

export async function injectFinalText(text: string) {
  if (!text) {
    cmdSilent(CMD.setIndicatorState, { state: "hidden" });
    pushDictLog("最终文本为空。");
    setDictationStatus("未识别到文本。", "err");
    playCue("end");
    return;
  }

  const finalText = await runLocalProcessing(text);
  cmdSilent(CMD.setIndicatorState, { state: "hidden" });
  useDictationStore.setState({ latestText: finalText });
  try {
    pushDictLog(`开始注入（方式=${getInjectMethod()}）…`);
    await cmd(CMD.injectText, { text: finalText, method: getInjectMethod() });
    pushDictLog("注入命令已执行完成。");
    setDictationStatus(
      `已注入：${finalText.slice(0, 40)}${finalText.length > 40 ? "…" : ""}`,
      "ok",
    );
  } catch (error) {
    pushDictLog(`注入失败：${String(error)}`);
    setDictationStatus(`注入失败：${String(error)}`, "err");
  }
  playCue("end");
}

export async function finalizeDictation() {
  if (dictSession.finalized || !dictSession.awaitingFinal) return;
  dictSession.finalized = true;
  dictSession.awaitingFinal = false;
  if (dictSession.finalizeTimer) {
    clearTimeout(dictSession.finalizeTimer);
    dictSession.finalizeTimer = null;
  }
  const text = (dictSession.committed + dictSession.segment).trim();
  pushDictLog(
    `收尾：最终 ${text.length} 字（累计段 ${dictSession.committed.length} + 当前段 ${dictSession.segment.length}），共 ${dictSession.resultCount} 条结果`,
  );
  const session = dictSession.sessionId;
  if (session) {
    await cmdSilent(CMD.stopAsrStream, { sessionId: session });
  }
  dictSession.sessionId = null;
  dictSession.mode = null;
  resetIndicatorPreview();
  await injectFinalText(text);
}
