import { useEffect } from "react";
import { EVT } from "@/lib/tauri";
import { useTauriEvent } from "./useTauriEvent";
import { useProviderStore } from "@/store/useProviderStore";
import { useSubtitleStore } from "@/store/useSubtitleStore";
import { syncDebugLogToBackend } from "@/store/useDictPrefs";
import {
  applyDictationRuntime,
  loadDictationRuntime,
  handleShortcutError,
  loadDictationSettings,
  isCapturing,
  shutdownDictationMic,
  handleCaptureLockKey,
} from "@/features/dictation/controller";
import {
  applySubtitleRuntime,
  loadSubtitleRuntime,
  shutdownSubtitles,
  handleSubtitleShortcutError,
  isSubtitleCapturing,
  loadSubtitleShortcut,
  installSubtitleFocusHotkeyFallback,
  handleForwardedSubtitleKeydown,
  handleForwardedSubtitleKeyup,
  handleSubtitleCaptureLockKey,
} from "@/features/subtitles/controller";
import { hardAbortCompare } from "@/features/compare/controller";

export function useTauriBridge() {
  useTauriEvent(EVT.domainEvent, (data) => {
    const event = (data || {}) as { domain?: string; payload?: unknown };
    if (event.domain === "dictation") applyDictationRuntime((event.payload || {}) as never);
    if (event.domain === "subtitles") applySubtitleRuntime((event.payload || {}) as never);
  });
  useTauriEvent(EVT.dictationShortcutError, (payload) => handleShortcutError(payload as never));

  useTauriEvent(EVT.subtitleCloseRequested, () => {
    shutdownSubtitles();
  });
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
    syncDebugLogToBackend();
    loadDictationSettings();
    void loadDictationRuntime().catch(() => undefined);
    loadSubtitleShortcut();
    void loadSubtitleRuntime().catch(() => undefined);
    void useSubtitleStore.getState().loadTranslationModel().catch(() => undefined);
    useProviderStore.getState().load();

    const uninstallSubtitleHotkeyFallback = installSubtitleFocusHotkeyFallback();
    const onUnload = () => {
      shutdownDictationMic();
      hardAbortCompare();
    };
    window.addEventListener("beforeunload", onUnload);
    return () => {
      uninstallSubtitleHotkeyFallback();
      window.removeEventListener("beforeunload", onUnload);
    };
  }, []);
}
