import { useEffect, useState } from "react";
import { CMD, EVT, cmd, on, type AppSnapshot, type DomainEventEnvelope } from "@/lib/tauri";
import { useTauriEvent } from "./useTauriEvent";
import { useProviderStore } from "@/store/useProviderStore";
import { useSubtitleStore } from "@/store/useSubtitleStore";
import { useCuePlayback } from "./useCuePlayback";
import { useFloatingOrbSync } from "./useFloatingOrbSync";
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
import { applyCompareRuntime, loadCompareRuntime } from "@/features/compare/controller";
import { applyAudioLabRuntime, loadAudioLabRuntime } from "@/features/audio/lab";

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
  useCuePlayback(EVT.dictationPlayCue, "main");
  useFloatingOrbSync();

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
    const uninstallSubtitleHotkeyFallback = installSubtitleFocusHotkeyFallback();
    const applyDomainEvent = (event: DomainEventEnvelope) => {
      if (event.domain === "dictation") applyDictationRuntime((event.payload || {}) as never);
      if (event.domain === "subtitles") applySubtitleRuntime((event.payload || {}) as never);
      if (event.domain === "transcription") applyTranscriptionRuntime((event.payload || {}) as never);
      if (event.domain === "comparison") applyCompareRuntime((event.payload || {}) as never);
      if (event.domain === "audioLab") applyAudioLabRuntime((event.payload || {}) as never);
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

        // 设置类 loader 一条失败不得拖垮后面的运行时投影恢复。
        //
        // 这批里只有 loadTranslationModel 会把错误招出去，而 `Promise.all` 会让它直接跳到
        // catch：监听各 runtime、revision 对账全部不再执行，界面停在空状态，只留一行
        // console.error。改成 allSettled：哪条挂了就报哪条，其余恢复照常跑完。
        const settingsLoads = await Promise.allSettled([
          loadDictationSettings(),
          loadSubtitleShortcut(),
          useSubtitleStore.getState().loadTranslationModel(),
          useProviderStore.getState().load(),
        ]);
        for (const result of settingsLoads) {
          if (result.status === "rejected") console.error("设置加载失败", result.reason);
        }

        // 运行时投影没有单独携带 revision，因此在稳定 revision 区间内加载；
        // 若加载期间发生领域变化就重试，避免较旧的命令响应覆盖刚收到的事件。
        for (let attempt = 0; attempt < 3; attempt += 1) {
          const before = await cmd<AppSnapshot>(CMD.getAppSnapshot);
          await Promise.all([loadDictationRuntime(), loadSubtitleRuntime(), loadTranscriptionRuntime(), loadCompareRuntime(), loadAudioLabRuntime()]);
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
