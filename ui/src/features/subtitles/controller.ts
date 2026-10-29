import { CMD, cmd, cmdSilent } from "@/lib/tauri";
import { useSubtitleStore, type SubtitlePrefs } from "@/store/useSubtitleStore";
import {
  configureSubtitleHotkeys,
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  handleSubtitleCaptureLockKey,
} from "./hotkeys";

export {
  startSubtitleShortcutCapture,
  clearSubtitleShortcut,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  handleSubtitleCaptureLockKey,
} from "./hotkeys";

export interface SubtitleRuntimeSnapshot {
  phase: "idle" | "waitingForVoice" | "running" | "reconnecting" | "stopping" | "failed";
  sessionId?: string;
  previewActive?: boolean;
  originalText: string;
  translationText: string;
  obsOutputActive: boolean;
  /** 致命失败：整个字幕会话已停止，伴随 phase === "failed"。 */
  error?: string;
  /** 翻译失败：字幕本身仍在滚动，只是译文出不来，phase 不变。 */
  translationError?: string;
}

function applyRuntime(snapshot: SubtitleRuntimeSnapshot) {
  const running = !["idle", "failed"].includes(snapshot.phase);
  const waiting = snapshot.phase === "waitingForVoice";
  const reconnecting = snapshot.phase === "reconnecting";
  const failed = snapshot.phase === "failed";
  useSubtitleStore.getState().setRuntime({
    running,
    previewActive: snapshot.previewActive === true,
    latestText: snapshot.originalText || "",
    obsOutputActive: snapshot.obsOutputActive === true,
    // 翻译失败是非致命的，phase 仍是 running/waitingForVoice。原先只在 failed 时
    // 展示 error，其余一律覆盖成「实时字幕已开启」，于是翻译供应商未启用/欠费时
    // 用户只看到译文永远空白 + 绿色的"已开启"，三处 UI 都没有任何线索。
    statusText: failed
      ? snapshot.error || "实时字幕运行失败"
      : snapshot.translationError
        ? snapshot.translationError
        : reconnecting
          ? "实时字幕重新连接中…"
          : waiting
            ? "实时字幕已开启，正在等待声音…"
            : running
              ? "实时字幕已开启"
              : "实时字幕未开启",
    statusTone: failed || snapshot.translationError ? "err" : running ? "ok" : "",
  });
}

export async function loadSubtitleRuntime() {
  applyRuntime(await cmd<SubtitleRuntimeSnapshot>(CMD.getSubtitleRuntime));
}

export function applySubtitleRuntime(snapshot: SubtitleRuntimeSnapshot) {
  applyRuntime(snapshot);
}

configureSubtitleHotkeys({
  setStatus: (statusText, statusTone = "") => useSubtitleStore.getState().setRuntime({ statusText, statusTone }),
  toggle: () => toggleSubtitles(),
});

export function handleSubtitleShortcutError(payload: { key_code?: string; message?: string }) {
  useSubtitleStore.getState().setRuntime({
    statusText: `实时字幕快捷键注册失败（${payload.key_code || "?"}）：${payload.message || "未知错误"}`,
    statusTone: "err",
  });
}

export async function toggleSubtitles() {
  try {
    await cmd(CMD.subtitleToggle);
    await loadSubtitleRuntime().catch(() => undefined);
  } catch (error) {
    useSubtitleStore.getState().setRuntime({ statusText: `实时字幕切换失败：${String(error)}`, statusTone: "err" });
  }
}

export async function shutdownSubtitles() {
  await cmdSilent(CMD.subtitleStop);
  await loadSubtitleRuntime().catch(() => undefined);
}

export async function applyObsOutputRouting() {
  await cmdSilent(CMD.applySubtitleObsRouting);
}

export async function syncSubtitleIndicator(prefs: SubtitlePrefs = useSubtitleStore.getState().prefs) {
  try {
    await cmd(CMD.syncSubtitlePresentation, { previewPrefs: prefs });
  } catch (error) {
    useSubtitleStore.getState().setRuntime({ statusText: `字幕样式更新失败：${String(error)}`, statusTone: "err" });
  }
}

export async function showSubtitlePreview(prefs: SubtitlePrefs) {
  if (useSubtitleStore.getState().running) return;
  try {
    await cmd(CMD.showSubtitlePreview, { prefs });
    await loadSubtitleRuntime();
  } catch (error) {
    useSubtitleStore.getState().setRuntime({ statusText: `字幕预览失败：${String(error)}`, statusTone: "err" });
  }
}

export async function hideSubtitlePreview() {
  try {
    await cmd(CMD.hideSubtitlePreview);
    await loadSubtitleRuntime();
  } catch (error) {
    useSubtitleStore.getState().setRuntime({ statusText: `关闭字幕预览失败：${String(error)}`, statusTone: "err" });
  }
}
