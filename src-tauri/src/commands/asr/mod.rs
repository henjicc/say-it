#[cfg(target_os = "macos")]
mod apple_session;
mod local_session;
mod plugin_session;
mod sdk_session;

use crate::commands::common::*;
use crate::prelude::*;
use crate::state::*;

pub(crate) async fn start_asr_stream_inner(
    app: tauri::AppHandle,
    state: &RuntimeState,
    provider_id: Option<String>,
    model_override: Option<String>,
    sample_rate: Option<u32>,
    params: Option<DspParams>,
) -> Result<AsrStreamStartResponse, String> {
    let provider_id = match provider_id {
        Some(provider_id) if !provider_id.trim().is_empty() => {
            resolve_provider_id(&state, "asr", Some(provider_id))?
        }
        _ => {
            let model_provider = model_override.as_deref().and_then(|model| {
                crate::providers::registry::model_info(model)
                    .map(|info| info.provider_id.clone())
                    .or_else(|| {
                        state
                            .plugin_registry
                            .lock()
                            .ok()
                            .and_then(|plugins| plugins.provider_id_for_model(model))
                    })
            });
            resolve_provider_id(&state, "asr", model_provider)?
        }
    };
    let profile = provider_profile_for_execution(&state, &provider_id)?;
    if profile.kind == crate::providers::apple_speech::PROVIDER_KIND {
        let model = model_override
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| "Apple 本地语音识别必须指定模型".to_string())?;
        #[cfg(target_os = "macos")]
        return apple_session::start_apple_speech_stream(
            app,
            state,
            model,
            sample_rate.unwrap_or(48_000),
            params,
        )
        .await;
        #[cfg(not(target_os = "macos"))]
        return Err("Apple 系统本地识别仅支持 macOS".into());
    }
    let local_model = model_override.as_deref().and_then(|model| {
        state
            .plugin_registry
            .lock()
            .ok()
            .and_then(|plugins| plugins.local_model_for_model(model))
    });
    if let Some(local_model) = local_model {
        let model = model_override
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| "本地识别必须指定模型".to_string())?;
        return local_session::start_local_asr_stream(
            app,
            state,
            local_model,
            model,
            sample_rate.unwrap_or(48_000),
            params,
        )
        .await;
    }
    if profile.kind == "sdk:bailian" {
        let model = model_override
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| "百炼实时识别必须指定模型".to_string())?;
        return sdk_session::start_bailian_sdk_stream(
            app,
            state,
            profile,
            model,
            sample_rate.unwrap_or(48_000),
            params,
        )
        .await;
    }
    let plugin = state
        .plugin_registry
        .lock()
        .map_err(|_| "插件注册表锁失败".to_string())?
        .runtime_for_provider(&provider_id)?
        .map(|spec| spec.bind_credentials(state.credentials.clone()));
    if let Some(plugin) = plugin {
        let model = model_override
            .filter(|model| !model.trim().is_empty())
            .ok_or_else(|| "插件实时识别必须指定模型".to_string())?;
        return plugin_session::start_plugin_asr_stream(
            app,
            state,
            plugin,
            profile,
            model,
            sample_rate.unwrap_or(48_000),
            params,
        )
        .await;
    }
    Err(format!(
        "实时识别供应商 {} 没有可注册的 SDK、插件或本地能力",
        profile.display_name
    ))
}

pub(crate) fn asr_stream_finish_inner(
    session_id: &str,
    state: &RuntimeState,
) -> Result<(), String> {
    let tx = state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .get(session_id)
        .ok_or_else(|| "ASR stream not found".to_string())?
        .tx
        .clone();
    tx.send(AsrStreamInput::Finish)
        .map_err(|_| "ASR stream channel closed".to_string())
}

pub(crate) fn stop_asr_stream_inner(session_id: &str, state: &RuntimeState) -> Result<(), String> {
    let handle = state
        .asr_streams
        .lock()
        .map_err(|_| "ASR stream lock failed".to_string())?
        .remove(session_id);
    if let Some(handle) = handle {
        let _ = handle.tx.send(AsrStreamInput::Stop);
    }
    Ok(())
}
