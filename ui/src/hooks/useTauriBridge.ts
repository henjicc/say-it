import { useEffect } from "react";
import { EVT } from "@/lib/tauri";
import { useTauriEvent } from "./useTauriEvent";
import { useProviderStore } from "@/store/useProviderStore";
import { syncDebugLogToBackend } from "@/store/useDictPrefs";
import {
  toggleDictation,
  onCancelKey,
  handleDictAsrEvent,
  handleDictTranscriptionEvent,
  handleShortcutError,
  loadDictationSettings,
  isCapturing,
  shutdownDictationMic,
  installFocusHotkeyFallback,
  handleForwardedKeydown,
  handleForwardedKeyup,
  handleCaptureLockKey,
} from "@/features/dictation/controller";
import {
  handleSubtitleAsrEvent,
  handleSubtitleTranslationEvent,
  shutdownSubtitles,
  toggleSubtitles,
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
  useTauriEvent(EVT.asrStreamEvent, (data) => {
    const payload = (data || {}) as { session_id?: string };
    if (!payload.session_id) return;
    if (handleSubtitleAsrEvent(payload as never)) return;
    handleDictAsrEvent(payload as never);
  });

  useTauriEvent(EVT.transcriptionEvent, (data) => {
    handleDictTranscriptionEvent((data || {}) as never);
  });

  useTauriEvent(EVT.dictationToggle, () => {
    if (isCapturing()) return;
    toggleDictation();
  });
  useTauriEvent(EVT.dictationCancel, () => {
    if (isCapturing()) return;
    onCancelKey();
  });
  useTauriEvent(EVT.dictationShortcutError, (payload) => handleShortcutError(payload as never));

  useTauriEvent(EVT.subtitleToggle, () => {
    if (isSubtitleCapturing()) return;
    toggleSubtitles();
  });
  useTauriEvent(EVT.subtitleShortcutError, (payload) => handleSubtitleShortcutError(payload as never));
  useTauriEvent(EVT.subtitleTranslationEvent, (payload) => handleSubtitleTranslationEvent(payload as never));

  useTauriEvent(EVT.hotkeyCaptureLockKey, (payload) => {
    const vk = ((payload || {}) as { vk?: number }).vk;
    if (typeof vk !== "number") return;
    handleCaptureLockKey(vk);
    handleSubtitleCaptureLockKey(vk);
  });

  useTauriEvent(EVT.indicatorKeydown, (payload) => {
    if (!isCapturing()) handleForwardedKeydown((payload || {}) as never);
    if (!isSubtitleCapturing()) handleForwardedSubtitleKeydown((payload || {}) as never);
  });
  useTauriEvent(EVT.indicatorKeyup, (payload) => {
    const code = ((payload || {}) as { code?: string }).code;
    handleForwardedKeyup(code);
    handleForwardedSubtitleKeyup(code);
  });

  useEffect(() => {
    syncDebugLogToBackend();
    loadDictationSettings();
    loadSubtitleShortcut();
    useProviderStore.getState().load();

    const uninstallHotkeyFallback = installFocusHotkeyFallback();
    const uninstallSubtitleHotkeyFallback = installSubtitleFocusHotkeyFallback();
    const onUnload = () => {
      shutdownSubtitles();
      shutdownDictationMic();
      hardAbortCompare();
    };
    window.addEventListener("beforeunload", onUnload);
    return () => {
      uninstallHotkeyFallback();
      uninstallSubtitleHotkeyFallback();
      window.removeEventListener("beforeunload", onUnload);
    };
  }, []);
}
