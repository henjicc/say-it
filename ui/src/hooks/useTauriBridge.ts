import { useEffect, useState } from "react";
import { CMD, EVT, cmd, on, type AppSnapshot, type DomainEventEnvelope } from "@/lib/tauri";
import { useTauriEvent } from "./useTauriEvent";
import { useProviderStore } from "@/store/useProviderStore";
import { useSubtitleStore } from "@/store/useSubtitleStore";
import { syncDebugLogToBackend } from "@/store/useDictPrefs";
import {
  applyDictationRuntime,
  loadDictationRuntime,
  handleShortcutError,
  loadDictationSettings,
  handleCaptureLockKey,
} from "@/features/dictation/controller";
import {
  applySubtitleRuntime,
  loadSubtitleRuntime,
  handleSubtitleShortcutError,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  handleSubtitleCaptureLockKey,
} from "@/features/subtitles/controller";
import { applyTranscriptionRuntime, loadTranscriptionRuntime } from "@/features/transcription/controller";

export function useTauriBridge() {
  const [ready, setReady] = useState(false);
  useTauriEvent(EVT.dictationShortcutError, (payload) => handleShortcutError(payload as never));

  useTauriEvent(EVT.subtitleShortcutError, (payload) => handleSubtitleShortcutError(payload as never));

  useTauriEvent(EVT.hotkeyCaptureLockKey, (payload) => {
    const vk = ((payload || {}) as { vk?: number }).vk;
    if (typeof vk !== "number") return;
    handleCaptureLockKey(vk);
    handleSubtitleCaptureLockKey(vk);
  });

  useTauriEvent(EVT.indicatorKeydown, (payload) => {
    if (!isSubtitleCapturing()) handleForwardedSubtitleKeydown((payload || {}) as never);
  });
  useTauriEvent(EVT.indicatorKeyup, (payload) => {
    const code = ((payload || {}) as { code?: string }).code;
    handleForwardedSubtitleKeyup(code);
  });

  useEffect(() => {
    let cancelled = false;
    let unlistenDomain: (() => void) | undefined;
    syncDebugLogToBackend();
    const uninstallSubtitleHotkeyFallback = installSubtitleFocusHotkeyFallback();
    const applyDomainEvent = (event: DomainEventEnvelope) => {
      if (event.domain === "dictation") applyDictationRuntime((event.payload || {}) as never);
      if (event.domain === "subtitles") applySubtitleRuntime((event.payload || {}) as never);
      if (event.domain === "transcription") applyTranscriptionRuntime((event.payload || {}) as never);
    };

    void (async () => {
      try {
        const baseline = await cmd<AppSnapshot>(CMD.getAppSnapshot);
        if (cancelled) return;
        let appliedRevision = baseline.revision;
        unlistenDomain = await on<DomainEventEnvelope>(EVT.domainEvent, (event) => {
          if (!Number.isFinite(event.revision) || event.revision <= appliedRevision) return;
          appliedRevision = event.revision;
          applyDomainEvent(event);
        });
        if (cancelled) {
          unlistenDomain();
          unlistenDomain = undefined;
          return;
        }

        await Promise.all([
          loadDictationSettings(),
          loadSubtitleShortcut(),
          useSubtitleStore.getState().loadTranslationModel(),
          useProviderStore.getState().load(),
        ]);

        // 运行时投影没有单独携带 revision，因此在稳定 revision 区间内加载；
        // 若加载期间发生领域变化就重试，避免较旧的命令响应覆盖刚收到的事件。
        for (let attempt = 0; attempt < 3; attempt += 1) {
          const before = await cmd<AppSnapshot>(CMD.getAppSnapshot);
          await Promise.all([loadDictationRuntime(), loadSubtitleRuntime(), loadTranscriptionRuntime()]);
          const corrected = await cmd<AppSnapshot>(CMD.getAppSnapshot);
          if (corrected.revision === before.revision) {
            appliedRevision = Math.max(appliedRevision, corrected.revision);
            break;
          }
        }
      } catch (error) {
        console.error("主窗口状态恢复失败", error);
      } finally {
        if (!cancelled) setReady(true);
      }
    })();

    return () => {
      cancelled = true;
      unlistenDomain?.();
      uninstallSubtitleHotkeyFallback();
    };
  }, []);

  return ready;
}
