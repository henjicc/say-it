import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type Event, type UnlistenFn } from "@tauri-apps/api/event";

export const CMD = {
  getAppSnapshot: "get_app_snapshot",
  getModelCatalog: "get_model_catalog",
  importLegacySettings: "import_legacy_settings",
  updateAppSettings: "update_app_settings",
  updateCustomCue: "update_custom_cue",
  getSessionStatus: "get_session_status",
  getDictationSettings: "get_dictation_settings",
  setDictationSettings: "set_dictation_settings",
  getSubtitleShortcut: "get_subtitle_shortcut",
  setSubtitleShortcut: "set_subtitle_shortcut",
  getSubtitleTranslationModel: "get_subtitle_translation_model",
  setSubtitleTranslationModel: "set_subtitle_translation_model",
  listProviders: "list_providers",
  getProviderSettings: "get_provider_settings",
  saveProviderSettings: "save_provider_settings",
  setDefaultProvider: "set_default_provider",
  updateProviderConfig: "update_provider_config",
  getProviderApiKey: "get_provider_api_key",
  funasrSaveHotwords: "funasr_save_hotwords",
  funasrSyncHotwords: "funasr_sync_hotwords",
  funasrClearHotwords: "funasr_clear_hotwords",
  providerSaveHotwords: "provider_save_hotwords",
  providerSyncHotwords: "provider_sync_hotwords",
  providerClearHotwords: "provider_clear_hotwords",
  startAsrStream: "start_asr_stream",
  stopAsrStream: "stop_asr_stream",
  asrStreamFinish: "asr_stream_finish",
  asrStreamPushF32Chunk: "asr_stream_push_f32_chunk",
  attachBackendMicToAsr: "attach_backend_mic_to_asr",
  attachBackendMicRawCapture: "attach_backend_mic_raw_capture",
  getBackendMicLevel: "get_backend_mic_level",
  startBackendMic: "start_backend_mic",
  pauseBackendMic: "pause_backend_mic",
  releaseBackendMic: "release_backend_mic",
  attachBackendSystemAudioToAsr: "attach_backend_system_audio_to_asr",
  attachBackendSystemAudioRawCapture: "attach_backend_system_audio_raw_capture",
  getBackendSystemAudioLevel: "get_backend_system_audio_level",
  startBackendSystemAudio: "start_backend_system_audio",
  pauseBackendSystemAudio: "pause_backend_system_audio",
  releaseBackendSystemAudio: "release_backend_system_audio",
  setIndicatorState: "set_indicator_state",
  setIndicatorText: "set_indicator_text",
  setIndicatorTranslation: "set_indicator_translation",
  setIndicatorLayout: "set_indicator_layout",
  getIndicatorMonitorMetrics: "get_indicator_monitor_metrics",
  translateSubtitleStart: "translate_subtitle_start",
  injectText: "inject_text",
  runAsrSilenceTest: "run_asr_silence_test",
  getLocalFileInfo: "get_local_file_info",
  saveTextFile: "save_text_file",
  transcriptionStart: "transcription_start",
  transcriptionCancel: "transcription_cancel",
  alignTranscript: "align_transcript",
  processAudioOffline: "process_audio_offline",
  openApiKeyPage: "open_api_key_page",
  openExternalLink: "open_external_link",
  getStartupSettings: "get_startup_settings",
  setStartupSettings: "set_startup_settings",
  setDebugLog: "set_debug_log",
  setHotkeyCapturing: "set_hotkey_capturing",
  debugModelCallState: "debug_model_call_state",
  listSystemFonts: "list_system_fonts",
  listAudioDevices: "list_audio_devices",
  encodeMonoWavFile: "encode_mono_wav_file",
  decodeAudioFilePcm: "decode_audio_file_pcm",
  getObsOverlayStatus: "get_obs_overlay_status",
  getObsConnectionSettings: "get_obs_connection_settings",
  getObsPassword: "get_obs_password",
  publishObsOverlaySnapshot: "publish_obs_overlay_snapshot",
  syncObsOverlayLayout: "sync_obs_overlay_layout",
  connectObs: "connect_obs",
  installObsOverlay: "install_obs_overlay",
  uninstallObsOverlay: "uninstall_obs_overlay",
} as const;

export type DomainRunState = "frontendOwned" | "idle" | "running" | "stopping" | "failed";

export interface DomainSnapshot {
  state: DomainRunState;
  sessionId?: string;
}

export interface AppSnapshot {
  revision: number;
  configuration: {
    defaultProviderId: string;
    dictationShortcut: string;
    subtitleShortcut: string;
    startupSilent: boolean;
  };
  settings: AppSettings;
  dictation: DomainSnapshot;
  subtitles: DomainSnapshot;
  transcription: DomainSnapshot;
  comparison: DomainSnapshot;
  audioLab: DomainSnapshot;
}

export interface AppSettings {
  schemaVersion: number;
  legacyImported: boolean;
  dictationPrefs: Record<string, unknown>;
  subtitlePrefs: Record<string, unknown>;
  comparePrefs: Record<string, unknown>;
  theme: Record<string, unknown>;
  customCueStart?: { relativePath: string; mimeType: string };
  customCueEnd?: { relativePath: string; mimeType: string };
}

export interface DomainEventEnvelope<T = unknown> {
  revision: number;
  domain: string;
  eventType: string;
  sessionId?: string;
  payload: T;
}

export const EVT = {
  asrStreamEvent: "asr-stream-event",
  transcriptionEvent: "transcription-event",
  dictationToggle: "dictation-toggle",
  dictationPressStart: "dictation-press-start",
  dictationPressEnd: "dictation-press-end",
  dictationCancel: "dictation-cancel",
  dictationShortcutError: "dictation-shortcut-error",
  subtitleToggle: "subtitle-toggle",
  subtitleCloseRequested: "subtitle-close-requested",
  subtitleShortcutError: "subtitle-shortcut-error",
  subtitleTranslationEvent: "subtitle-translation-event",
  hotkeyCaptureLockKey: "hotkey-capture-lock-key",
  indicatorState: "dictation-indicator-state",
  indicatorText: "dictation-indicator-text",
  indicatorTranslation: "dictation-indicator-translation",
  indicatorWaveform: "dictation-indicator-waveform",
  indicatorConfig: "dictation-indicator-config",
  indicatorKeydown: "dictation-indicator-keydown",
  indicatorKeyup: "dictation-indicator-keyup",
  backendMicRawChunk: "backend-mic-raw-chunk",
  backendMicPreviewChunk: "backend-mic-preview-chunk",
  backendMicRawEnded: "backend-mic-raw-ended",
  backendSystemAudioRawChunk: "backend-system-audio-raw-chunk",
  backendSystemAudioRawEnded: "backend-system-audio-raw-ended",
} as const;

export function cmd<T = unknown>(
  command: string,
  args?: Record<string, unknown>,
): Promise<T> {
  return invoke<T>(command, args);
}

export function cmdSilent(command: string, args?: Record<string, unknown>): Promise<void> {
  return invoke(command, args).then(
    () => undefined,
    () => undefined,
  );
}

export function on<T = unknown>(
  event: string,
  handler: (payload: T, raw: Event<T>) => void,
): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload, e));
}

export function emitEvent(event: string, payload?: unknown): Promise<void> {
  return emit(event, payload);
}
