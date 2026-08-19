use crate::persistence::save_persisted_state_with_app_settings;
use crate::state::RuntimeState;
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State};

pub(crate) const ONBOARDING_VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupCheckResult {
    id: String,
    status: String,
    title: String,
    message: String,
    action: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetupStatus {
    onboarding_version: u32,
    required_version: u32,
    complete: bool,
    checks: Vec<SetupCheckResult>,
}

fn check(
    id: &str,
    status: &str,
    title: &str,
    message: impl Into<String>,
    action: Option<&str>,
) -> SetupCheckResult {
    SetupCheckResult {
        id: id.into(),
        status: status.into(),
        title: title.into(),
        message: message.into(),
        action: action.map(str::to_string),
    }
}

fn microphone_check() -> SetupCheckResult {
    match crate::desktop::list_audio_devices() {
        Ok(devices) if !devices.inputs.is_empty() => check(
            "microphone",
            "ready",
            "麦克风",
            format!("检测到 {} 个输入设备", devices.inputs.len()),
            None,
        ),
        Ok(_) => check(
            "microphone",
            "blocked",
            "麦克风",
            "没有检测到输入设备",
            Some("audio"),
        ),
        Err(error) => check("microphone", "blocked", "麦克风", error, Some("audio")),
    }
}

fn permission_check() -> SetupCheckResult {
    #[cfg(target_os = "macos")]
    {
        let permissions = crate::macos_native::context_ocr_permissions(false);
        let ready = permissions.accessibility;
        let message = match (permissions.accessibility, permissions.screen_recording) {
            (true, true) => "辅助功能与屏幕录制权限均已授予".to_string(),
            (true, false) => "辅助功能可用；启用窗口 OCR 时还需要屏幕录制权限".to_string(),
            (false, _) => "需要在系统设置中授予辅助功能权限".to_string(),
        };
        return check(
            "permissions",
            if ready { "ready" } else { "blocked" },
            "系统权限",
            message,
            (!ready).then_some("permissions"),
        );
    }
    #[cfg(windows)]
    {
        check(
            "permissions",
            "ready",
            "系统权限",
            "Windows 基础听写无需额外辅助功能授权",
            None,
        )
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        check(
            "permissions",
            "warning",
            "系统权限",
            "当前平台未纳入本阶段支持范围",
            None,
        )
    }
}

fn provider_check(state: &RuntimeState) -> SetupCheckResult {
    let (configured, incomplete) = state
        .providers
        .lock()
        .ok()
        .map(|settings| {
            settings
                .profiles
                .iter()
                .filter(|profile| {
                    profile.enabled
                        && profile
                            .capabilities
                            .iter()
                            .any(|capability| capability == "asr")
                        && (profile.kind != crate::providers::apple_speech::PROVIDER_KIND
                            || crate::providers::apple_speech::runtime_available())
                })
                .fold((0usize, 0usize), |(ready, incomplete), profile| {
                    if profile.kind == crate::providers::apple_speech::PROVIDER_KIND {
                        let status = crate::providers::apple_speech::status();
                        return if status.available {
                            (ready + 1, incomplete)
                        } else {
                            (ready, incomplete + 1)
                        };
                    }
                    let requires_key = profile.auth_kind == "api-key";
                    let has_key = profile
                        .config
                        .get("apiKey")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty());
                    if !requires_key || has_key || profile.kind.starts_with("plugin:") {
                        (ready + 1, incomplete)
                    } else {
                        (ready, incomplete + 1)
                    }
                })
        })
        .unwrap_or_default();
    if configured > 0 {
        check(
            "provider",
            "ready",
            "识别能力",
            format!("检测到 {configured} 个已启用的语音识别服务或本地引擎"),
            None,
        )
    } else if incomplete > 0 {
        check(
            "provider",
            "blocked",
            "识别能力",
            format!("检测到 {incomplete} 个已启用服务，但认证信息尚未配置完整"),
            Some("model"),
        )
    } else {
        check(
            "provider",
            "blocked",
            "识别能力",
            "尚未启用语音识别服务或本地模型",
            Some("model"),
        )
    }
}

fn shortcut_check(state: &RuntimeState) -> SetupCheckResult {
    match state.dictation.lock() {
        Ok(settings) if !settings.key_code.trim().is_empty() => check(
            "shortcut",
            "ready",
            "主快捷键",
            format!("已设置 {}", settings.key_code),
            None,
        ),
        Ok(_) => check(
            "shortcut",
            "blocked",
            "主快捷键",
            "主听写快捷键尚未设置",
            Some("keys"),
        ),
        Err(_) => check(
            "shortcut",
            "blocked",
            "主快捷键",
            "读取快捷键配置失败",
            Some("keys"),
        ),
    }
}

fn collect(state: &RuntimeState) -> Vec<SetupCheckResult> {
    let mut checks = vec![microphone_check()];
    // Windows 基础听写没有需要用户处理的授权步骤，不在首次引导里展示一条
    // “已通过”的技术性权限检查。macOS 和尚未正式支持的平台仍保留检查。
    #[cfg(not(windows))]
    checks.push(permission_check());
    checks.push(provider_check(state));
    checks.push(shortcut_check(state));
    checks
}

fn pcm16_rms(bytes: &[u8]) -> f32 {
    if bytes.len() < 2 {
        return 0.0;
    }
    let (sum, count) = bytes
        .chunks_exact(2)
        .fold((0.0f32, 0usize), |(sum, count), pair| {
            let value = i16::from_le_bytes([pair[0], pair[1]]) as f32 / 32768.0;
            (sum + value * value, count + 1)
        });
    (sum / count as f32).sqrt()
}

#[tauri::command]
pub(crate) fn start_setup_mic_meter(
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<(), String> {
    let sample_rate = state
        .backend_mic
        .lock()
        .map_err(|_| "麦克风状态锁失败")?
        .sample_rate;
    if sample_rate == 0 {
        return Err("麦克风尚未启动".to_string());
    }
    let params = serde_json::from_value::<crate::audio_dsp::DspParams>(
        state
            .app_settings
            .lock()
            .map_err(|_| "应用配置锁失败")?
            .dictation_prefs
            .clone(),
    )
    .map_err(|error| format!("音频处理配置无效：{error}"))?;
    let (_, mut receiver) = crate::desktop::attach_backend_mic_raw_inner(&state)?;
    let epoch = state.setup_mic_meter_epoch.fetch_add(1, Ordering::AcqRel) + 1;
    state.setup_mic_level_bits.store(0, Ordering::Release);

    tauri::async_runtime::spawn(async move {
        let mut dsp = crate::audio_dsp::StreamDsp::new(params, sample_rate);
        while let Some(input) = receiver.recv().await {
            let crate::state::AsrStreamInput::RawF32(samples) = input else {
                continue;
            };
            let state = app.state::<RuntimeState>();
            if state.setup_mic_meter_epoch.load(Ordering::Acquire) != epoch {
                break;
            }
            let processed = dsp.process(&samples);
            if !processed.is_empty() {
                state
                    .setup_mic_level_bits
                    .store(pcm16_rms(&processed).to_bits(), Ordering::Release);
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) fn get_setup_mic_level(state: State<'_, RuntimeState>) -> f32 {
    f32::from_bits(state.setup_mic_level_bits.load(Ordering::Acquire))
}

#[tauri::command]
pub(crate) fn stop_setup_mic_meter(state: State<'_, RuntimeState>) {
    state.setup_mic_meter_epoch.fetch_add(1, Ordering::AcqRel);
    state.setup_mic_level_bits.store(0, Ordering::Release);
}

fn persist_result(
    app: &AppHandle,
    state: &State<'_, RuntimeState>,
    result: &SetupCheckResult,
) -> Result<(), String> {
    let mut next = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .clone();
    if !next.setup_results.is_object() {
        next.setup_results = serde_json::json!({});
    }
    next.setup_results[&result.id] = serde_json::json!({
        "version": ONBOARDING_VERSION,
        "checkedAt": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
        "result": result,
    });
    save_persisted_state_with_app_settings(app, state, Some(&next))?;
    *state.app_settings.lock().map_err(|_| "应用配置锁失败")? = next;
    Ok(())
}

#[tauri::command]
pub(crate) fn get_setup_status(state: State<'_, RuntimeState>) -> Result<SetupStatus, String> {
    let onboarding_version = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .onboarding_version;
    let checks = collect(&state);
    Ok(SetupStatus {
        onboarding_version,
        required_version: ONBOARDING_VERSION,
        complete: onboarding_version >= ONBOARDING_VERSION,
        checks,
    })
}

#[tauri::command]
pub(crate) fn run_setup_check(
    app: AppHandle,
    id: String,
    state: State<'_, RuntimeState>,
) -> Result<SetupCheckResult, String> {
    let result = match id.as_str() {
        "microphone" => Ok(microphone_check()),
        "permissions" => Ok(permission_check()),
        "provider" => Ok(provider_check(&state)),
        "shortcut" => Ok(shortcut_check(&state)),
        _ => Err(format!("未知体检项目：{id}")),
    }?;
    persist_result(&app, &state, &result)?;
    Ok(result)
}

#[tauri::command]
pub(crate) fn request_setup_permissions(
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<SetupCheckResult, String> {
    #[cfg(target_os = "macos")]
    {
        let _ = crate::macos_native::prepare_accessibility_permission(true);
    }
    let result = permission_check();
    persist_result(&app, &state, &result)?;
    Ok(result)
}

#[tauri::command]
pub(crate) async fn run_injection_setup_check(
    app: AppHandle,
    state: State<'_, RuntimeState>,
    text: String,
) -> Result<SetupCheckResult, String> {
    let marker = if text.trim().is_empty() {
        "说吧！注入测试".to_string()
    } else {
        text
    };
    crate::commands::dictation::inject_text_inner(marker, Some("paste".into())).await?;
    let result = check(
        "injection",
        "ready",
        "文本注入",
        "测试文本已发送到当前输入框",
        None,
    );
    persist_result(&app, &state, &result)?;
    Ok(result)
}

#[tauri::command]
pub(crate) fn complete_onboarding(
    app: AppHandle,
    state: State<'_, RuntimeState>,
) -> Result<SetupStatus, String> {
    let mut next = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败")?
        .clone();
    next.onboarding_version = ONBOARDING_VERSION;
    if !next.setup_results.is_object() {
        next.setup_results = serde_json::json!({});
    }
    for result in collect(&state) {
        next.setup_results[&result.id] = serde_json::json!({
            "version": ONBOARDING_VERSION,
            "result": result,
        });
    }
    save_persisted_state_with_app_settings(&app, &state, Some(&next))?;
    *state.app_settings.lock().map_err(|_| "应用配置锁失败")? = next;
    get_setup_status(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processed_pcm_level_uses_normalized_samples() {
        let samples = [0i16, 16_384, -16_384];
        let bytes: Vec<u8> = samples.into_iter().flat_map(i16::to_le_bytes).collect();
        let expected = (0.5f32 / 3.0).sqrt();
        assert!((pcm16_rms(&bytes) - expected).abs() < 0.0001);
    }
}
