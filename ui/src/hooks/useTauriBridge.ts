import { useEffect } from "react";
import { EVT } from "@/lib/tauri";
import { useTauriEvent } from "./useTauriEvent";
import { useProviderStore } from "@/store/useProviderStore";
import { syncDebugLogToBackend } from "@/store/useDictPrefs";
import {
  toggleDictation,
  onCancelKey,
  handleDictAsrEvent,
  handleShortcutError,
  loadDictationSettings,
  isCapturing,
  shutdownDictationMic,
  installFocusHotkeyFallback,
  handleForwardedKeydown,
  handleForwardedKeyup,
} from "@/features/dictation/controller";

export function useTauriBridge() {
  useTauriEvent(EVT.asrStreamEvent, (data) => {
    const payload = (data || {}) as { session_id?: string };
    if (!payload.session_id) return;
    handleDictAsrEvent(payload as never);
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

  useTauriEvent(EVT.indicatorKeydown, (payload) => {
    if (isCapturing()) return;
    handleForwardedKeydown((payload || {}) as never);
  });
  useTauriEvent(EVT.indicatorKeyup, (payload) =>
    handleForwardedKeyup(((payload || {}) as { code?: string }).code),
  );

  useEffect(() => {
    syncDebugLogToBackend();
    loadDictationSettings();
    useProviderStore.getState().load();

    const uninstallHotkeyFallback = installFocusHotkeyFallback();
    const onUnload = () => {
      shutdownDictationMic();
    };
    window.addEventListener("beforeunload", onUnload);
    return () => {
      uninstallHotkeyFallback();
      window.removeEventListener("beforeunload", onUnload);
    };
  }, []);
}
