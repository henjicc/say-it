import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type Event, type UnlistenFn } from "@tauri-apps/api/event";

export const CMD = {
  getAppSnapshot: "get_app_snapshot",
  mainWindowReady: "main_window_ready",
  getModelCatalog: "get_model_catalog",
  listProviderPlugins: "list_provider_plugins",
  reloadProviderPlugins: "reload_provider_plugins",
  installProviderPlugin: "install_provider_plugin",
  downloadProviderModelPack: "download_provider_model_pack",
  setProviderPluginEnabled: "set_provider_plugin_enabled",
  uninstallProviderPlugin: "uninstall_provider_plugin",
  runProviderPluginAction: "run_provider_plugin_action",
  importLegacySettings: "import_legacy_settings",
  updateAppSettings: "update_app_settings",
  updateCustomCue: "update_custom_cue",
  getSessionStatus: "get_session_status",
  getDictationSettings: "get_dictation_settings",
  setDictationSettings: "set_dictation_settings",
  getDictationRuntime: "get_dictation_runtime",
  getTranscriptionRuntime: "get_transcription_runtime",
  audioLabStart: "audio_lab_start",
  audioLabStop: "audio_lab_stop",
  audioLabReprocess: "audio_lab_reprocess",
  getAudioLabRuntime: "get_audio_lab_runtime",
  audioLabAudioPath: "audio_lab_audio_path",
  compareStart: "compare_start",
  compareStop: "compare_stop",
  compareCancel: "compare_cancel",
  getCompareRuntime: "get_compare_runtime",
  dictationToggle: "dictation_toggle",
  dictationStart: "dictation_start",
  dictationStop: "dictation_stop",
  dictationCancel: "dictation_cancel",
  previewDictationCue: "preview_dictation_cue",
  getSubtitleShortcut: "get_subtitle_shortcut",
  setSubtitleShortcut: "set_subtitle_shortcut",
  getSubtitleTranslationModel: "get_subtitle_translation_model",
  setSubtitleTranslationModel: "set_subtitle_translation_model",
  getSubtitleRuntime: "get_subtitle_runtime",
  subtitleToggle: "subtitle_toggle",
  subtitleStop: "subtitle_stop",
  syncSubtitlePresentation: "sync_subtitle_presentation",
  applySubtitleObsRouting: "apply_subtitle_obs_routing",
  listProviders: "list_providers",
  setDefaultProvider: "set_default_provider",
  updateProviderConfig: "update_provider_config",
  getProviderApiKey: "get_provider_api_key",
  addLlmProvider: "add_llm_provider",
  refreshLlmModels: "refresh_llm_models",
  removeLlmProvider: "remove_llm_provider",
  previewSmartText: "preview_smart_text",
  providerSaveHotwords: "provider_save_hotwords",
  providerSyncHotwords: "provider_sync_hotwords",
  providerClearHotwords: "provider_clear_hotwords",
  getBackendMicLevel: "get_backend_mic_level",
  startBackendMic: "start_backend_mic",
  releaseBackendMic: "release_backend_mic",
  getBackendSystemAudioLevel: "get_backend_system_audio_level",
  startBackendSystemAudio: "start_backend_system_audio",
  releaseBackendSystemAudio: "release_backend_system_audio",
  setIndicatorState: "set_indicator_state",
  setIndicatorText: "set_indicator_text",
  setIndicatorTranslation: "set_indicator_translation",
  setIndicatorLayout: "set_indicator_layout",
  getIndicatorMonitorMetrics: "get_indicator_monitor_metrics",
  openActiveAppContextDebug: "open_active_app_context_debug",
  closeActiveAppContextDebug: "close_active_app_context_debug",
  setActiveAppContextDebugOverrides: "set_active_app_context_debug_overrides",
  getLocalFileInfo: "get_local_file_info",
  saveSubtitleSrt: "save_subtitle_srt",
  transcriptionStart: "transcription_start",
  transcriptionCancel: "transcription_cancel",
  alignTranscript: "align_transcript",
  openApiKeyPage: "open_api_key_page",
  openExternalLink: "open_external_link",
  getStartupSettings: "get_startup_settings",
  setStartupSettings: "set_startup_settings",
  getDataRootStatus: "get_data_root_status",
  migrateDataRoot: "migrate_data_root",
  restartApp: "restart_app",
  setDebugLog: "set_debug_log",
  setHotkeyCapturing: "set_hotkey_capturing",
  debugModelCallState: "debug_model_call_state",
  listSystemFonts: "list_system_fonts",
  listAudioDevices: "list_audio_devices",
  getObsOverlayStatus: "get_obs_overlay_status",
  getObsConnectionSettings: "get_obs_connection_settings",
  getObsPassword: "get_obs_password",
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
  domainEvent: "domain-event",
  transcriptionEvent: "transcription-event",
  dictationShortcutError: "dictation-shortcut-error",
  subtitleCloseRequested: "subtitle-close-requested",
  subtitleShortcutError: "subtitle-shortcut-error",
  hotkeyCaptureLockKey: "hotkey-capture-lock-key",
  dictationPlayCue: "dictation-play-cue",
  indicatorPlayCue: "dictation-indicator-play-cue",
  indicatorState: "dictation-indicator-state",
  indicatorText: "dictation-indicator-text",
  indicatorTranslation: "dictation-indicator-translation",
  indicatorWaveform: "dictation-indicator-waveform",
  indicatorConfig: "dictation-indicator-config",
  indicatorKeydown: "dictation-indicator-keydown",
  indicatorKeyup: "dictation-indicator-keyup",
  contextDebugState: "active-app-context-debug-state",
  contextDebugResult: "active-app-context-debug-result",
  dataRootMigration: "data-root-migration",
  modelPackProgress: "model-pack-progress",
  pluginInstallProgress: "plugin-install-progress",
} as const;

export interface DataRootStatus {
  activeRoot: string;
  configuredRoot: string;
  defaultRoot: string;
  isCustom: boolean;
  restartRequired: boolean;
}

export interface DataRootMigrationEvent {
  phase: "copying" | "done" | "failed";
  copiedBytes: number;
  totalBytes: number;
  copiedFiles: number;
  totalFiles: number;
  message?: string | null;
}

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
