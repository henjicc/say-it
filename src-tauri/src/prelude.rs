pub(crate) use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

pub(crate) use crate::audio_dsp::{process_offline, DspParams, OfflineResult, StreamDsp, OUTPUT_RATE};
pub(crate) use crate::{debug_log_enabled, dlog, hotkey};
pub(crate) use crate::providers::{
    default_provider_id, find_profile, has_capability, normalize_settings, sanitized_config,
    set_default_provider as set_default_provider_value, ProviderListItem, ProviderProfile,
    ProviderSettings, ProviderSettingsResponse, ProviderStatus, SetDefaultProviderRequest,
    FUNASR_PROVIDER_ID,
};
pub(crate) use crate::providers::alibabacloud::{
    build_finish_task_message, build_run_task_message, create_vocabulary as funasr_create_vocabulary,
    delete_vocabulary as funasr_delete_vocabulary, parse_server_message as parse_funasr_message,
    update_vocabulary as funasr_update_vocabulary, ws_request as funasr_ws_request, FunAsrEvent,
    FunAsrParams, HotwordEntry,
};
pub(crate) use base64::{engine::general_purpose::STANDARD, Engine as _};
pub(crate) use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
pub(crate) use enigo::{Direction, Enigo, Key, Keyboard, Settings as EnigoSettings};
pub(crate) use futures_util::{SinkExt, StreamExt};
pub(crate) use serde::{Deserialize, Serialize};
pub(crate) use serde_json::{json, Value};
pub(crate) use tauri::{
    menu::MenuBuilder,
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};
pub(crate) use tauri_plugin_autostart::ManagerExt;
pub(crate) use tokio::time::sleep;
pub(crate) use tokio_tungstenite::{connect_async, tungstenite::Message};
pub(crate) use uuid::Uuid;
