use crate::persistence::save_persisted_state;
use crate::prelude::*;
use crate::state::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ShortcutTarget {
    DictationMain,
    DictationProfile {
        #[serde(rename = "profileId")]
        profile_id: String,
    },
    Subtitles,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShortcutBindingInput {
    pub(crate) key_code: String,
    pub(crate) ctrl: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    pub(crate) meta: bool,
    pub(crate) trigger_mode: ShortcutTriggerMode,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShortcutBindingItem {
    pub(crate) target: ShortcutTarget,
    pub(crate) name: String,
    pub(crate) action_label: String,
    pub(crate) enabled: bool,
    pub(crate) key_code: String,
    pub(crate) ctrl: bool,
    pub(crate) shift: bool,
    pub(crate) alt: bool,
    pub(crate) meta: bool,
    pub(crate) trigger_mode: ShortcutTriggerMode,
    pub(crate) trigger_mode_editable: bool,
}

#[tauri::command]
pub(crate) fn get_shortcut_bindings(
    state: tauri::State<'_, RuntimeState>,
) -> Result<Vec<ShortcutBindingItem>, String> {
    let _operation = state
        .shortcut_config_operation
        .lock()
        .map_err(|_| "快捷键配置操作锁失败".to_string())?;
    let dictation = state
        .dictation
        .lock()
        .map_err(|_| "Dictation lock failed".to_string())?
        .clone();
    let subtitle = state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "Subtitle shortcut lock failed".to_string())?
        .clone();
    Ok(collect_shortcut_bindings(&dictation, &subtitle))
}

#[tauri::command]
pub(crate) fn update_shortcut_binding(
    app: tauri::AppHandle,
    target: ShortcutTarget,
    binding: ShortcutBindingInput,
    state: tauri::State<'_, RuntimeState>,
) -> Result<Vec<ShortcutBindingItem>, String> {
    transact_shortcut_settings(&app, &state, move |dictation, subtitle| {
        apply_binding_update(dictation, subtitle, &target, binding)
    })
}

#[tauri::command]
pub(crate) fn clear_shortcut_binding(
    app: tauri::AppHandle,
    target: ShortcutTarget,
    state: tauri::State<'_, RuntimeState>,
) -> Result<Vec<ShortcutBindingItem>, String> {
    transact_shortcut_settings(&app, &state, move |dictation, subtitle| {
        clear_binding(dictation, subtitle, &target)
    })
}

pub(crate) fn replace_dictation_settings(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    settings: DictationSettings,
) -> Result<(), String> {
    transact_shortcut_settings(app, state, move |dictation, _| {
        *dictation = settings;
        Ok(())
    })
    .map(|_| ())
}

pub(crate) fn replace_subtitle_shortcut(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    settings: SubtitleShortcutSettings,
) -> Result<(), String> {
    transact_shortcut_settings(app, state, move |_, subtitle| {
        *subtitle = settings;
        Ok(())
    })
    .map(|_| ())
}

fn transact_shortcut_settings<F>(
    app: &tauri::AppHandle,
    state: &tauri::State<'_, RuntimeState>,
    mutate: F,
) -> Result<Vec<ShortcutBindingItem>, String>
where
    F: FnOnce(&mut DictationSettings, &mut SubtitleShortcutSettings) -> Result<(), String>,
{
    let _operation = state
        .shortcut_config_operation
        .lock()
        .map_err(|_| "快捷键配置操作锁失败".to_string())?;
    let previous_dictation = state
        .dictation
        .lock()
        .map_err(|_| "Dictation lock failed".to_string())?
        .clone();
    let previous_subtitle = state
        .subtitle_shortcut
        .lock()
        .map_err(|_| "Subtitle shortcut lock failed".to_string())?
        .clone();
    let mut next_dictation = previous_dictation.clone();
    let mut next_subtitle = previous_subtitle.clone();
    mutate(&mut next_dictation, &mut next_subtitle)?;
    validate_shortcut_settings(&next_dictation, &next_subtitle, state)?;

    if let Err(error) = apply_dictation_hotkey(&next_dictation) {
        return Err(error);
    }
    if let Err(error) = apply_subtitle_hotkey(&next_subtitle) {
        return Err(restore_after_registration_failure(
            error,
            &previous_dictation,
            &previous_subtitle,
        ));
    }

    let state_update = (|| -> Result<(), String> {
        let mut dictation = state
            .dictation
            .lock()
            .map_err(|_| "Dictation lock failed".to_string())?;
        let mut subtitle = state
            .subtitle_shortcut
            .lock()
            .map_err(|_| "Subtitle shortcut lock failed".to_string())?;
        *dictation = next_dictation.clone();
        *subtitle = next_subtitle.clone();
        Ok(())
    })();
    if let Err(error) = state_update {
        return Err(restore_after_registration_failure(
            error,
            &previous_dictation,
            &previous_subtitle,
        ));
    }

    if let Err(error) = save_persisted_state(app, state) {
        let mut failures = restore_runtime_state(state, &previous_dictation, &previous_subtitle);
        failures.extend(restore_registrations(
            &previous_dictation,
            &previous_subtitle,
        ));
        return Err(with_restore_failures(error, failures));
    }

    Ok(collect_shortcut_bindings(&next_dictation, &next_subtitle))
}

fn restore_after_registration_failure(
    error: String,
    dictation: &DictationSettings,
    subtitle: &SubtitleShortcutSettings,
) -> String {
    with_restore_failures(error, restore_registrations(dictation, subtitle))
}

fn restore_registrations(
    dictation: &DictationSettings,
    subtitle: &SubtitleShortcutSettings,
) -> Vec<String> {
    let mut restore_errors = Vec::new();
    if let Err(restore) = apply_dictation_hotkey(dictation) {
        restore_errors.push(format!("恢复语音输入快捷键失败：{restore}"));
    }
    if let Err(restore) = apply_subtitle_hotkey(subtitle) {
        restore_errors.push(format!("恢复字幕快捷键失败：{restore}"));
    }
    restore_errors
}

fn restore_runtime_state(
    state: &RuntimeState,
    dictation: &DictationSettings,
    subtitle: &SubtitleShortcutSettings,
) -> Vec<String> {
    let mut restore_errors = Vec::new();
    match state.dictation.lock() {
        Ok(mut current) => *current = dictation.clone(),
        Err(_) => restore_errors.push("恢复语音输入配置内存失败".to_string()),
    }
    match state.subtitle_shortcut.lock() {
        Ok(mut current) => *current = subtitle.clone(),
        Err(_) => restore_errors.push("恢复字幕快捷键配置内存失败".to_string()),
    }
    restore_errors
}

fn with_restore_failures(error: String, restore_errors: Vec<String>) -> String {
    if restore_errors.is_empty() {
        error
    } else {
        format!("{error}；{}", restore_errors.join("；"))
    }
}

fn apply_binding_update(
    dictation: &mut DictationSettings,
    subtitle: &mut SubtitleShortcutSettings,
    target: &ShortcutTarget,
    mut binding: ShortcutBindingInput,
) -> Result<(), String> {
    binding.key_code = binding.key_code.trim().to_string();
    if binding.key_code.is_empty() {
        return Err("快捷键不能为空；如需移除请使用清除操作".to_string());
    }
    match target {
        ShortcutTarget::DictationMain => {
            dictation.key_code = binding.key_code;
            dictation.ctrl = binding.ctrl;
            dictation.shift = binding.shift;
            dictation.alt = binding.alt;
            dictation.meta = binding.meta;
            dictation.press_hold_mode = binding.trigger_mode == ShortcutTriggerMode::PressHold;
        }
        ShortcutTarget::DictationProfile { profile_id } => {
            let profile = dictation
                .shortcut_profiles
                .iter_mut()
                .find(|profile| profile.id == *profile_id)
                .ok_or_else(|| "对应的快捷键方案已不存在，请刷新后重试".to_string())?;
            profile.key_code = binding.key_code;
            profile.ctrl = binding.ctrl;
            profile.shift = binding.shift;
            profile.alt = binding.alt;
            profile.meta = binding.meta;
            profile.trigger_mode = binding.trigger_mode;
        }
        ShortcutTarget::Subtitles => {
            if binding.trigger_mode != ShortcutTriggerMode::Toggle {
                return Err("实时字幕仅支持单击切换".to_string());
            }
            subtitle.key_code = binding.key_code;
            subtitle.ctrl = binding.ctrl;
            subtitle.shift = binding.shift;
            subtitle.alt = binding.alt;
            subtitle.meta = binding.meta;
        }
    }
    Ok(())
}

fn clear_binding(
    dictation: &mut DictationSettings,
    subtitle: &mut SubtitleShortcutSettings,
    target: &ShortcutTarget,
) -> Result<(), String> {
    match target {
        ShortcutTarget::DictationMain => clear_combo(
            &mut dictation.key_code,
            &mut dictation.ctrl,
            &mut dictation.shift,
            &mut dictation.alt,
            &mut dictation.meta,
        ),
        ShortcutTarget::DictationProfile { profile_id } => {
            let profile = dictation
                .shortcut_profiles
                .iter_mut()
                .find(|profile| profile.id == *profile_id)
                .ok_or_else(|| "对应的快捷键方案已不存在，请刷新后重试".to_string())?;
            clear_combo(
                &mut profile.key_code,
                &mut profile.ctrl,
                &mut profile.shift,
                &mut profile.alt,
                &mut profile.meta,
            );
            profile.enabled = false;
        }
        ShortcutTarget::Subtitles => clear_combo(
            &mut subtitle.key_code,
            &mut subtitle.ctrl,
            &mut subtitle.shift,
            &mut subtitle.alt,
            &mut subtitle.meta,
        ),
    }
    Ok(())
}

fn clear_combo(
    key_code: &mut String,
    ctrl: &mut bool,
    shift: &mut bool,
    alt: &mut bool,
    meta: &mut bool,
) {
    key_code.clear();
    *ctrl = false;
    *shift = false;
    *alt = false;
    *meta = false;
}

pub(crate) fn validate_shortcut_settings(
    dictation: &DictationSettings,
    subtitle: &SubtitleShortcutSettings,
    state: &RuntimeState,
) -> Result<(), String> {
    if dictation.shortcut_profiles.len() > MAX_DICTATION_SHORTCUT_PROFILES {
        return Err(format!(
            "快捷键方案不能超过 {MAX_DICTATION_SHORTCUT_PROFILES} 条"
        ));
    }
    let known_templates = state
        .app_settings
        .lock()
        .map_err(|_| "应用配置锁失败".to_string())?
        .dictation_prefs
        .get("smartTemplates")
        .and_then(serde_json::Value::as_array)
        .map(|templates| {
            templates
                .iter()
                .filter_map(|template| template.get("id").and_then(serde_json::Value::as_str))
                .map(str::to_string)
                .collect::<std::collections::HashSet<_>>()
        })
        .unwrap_or_default();

    let mut ids = std::collections::HashSet::new();
    let mut shortcuts = std::collections::HashMap::<(u16, u8, bool), String>::new();
    if !dictation.key_code.trim().is_empty() {
        let vk = hotkey::code_to_vk(&dictation.key_code)
            .ok_or_else(|| format!("不支持的按键：{}", dictation.key_code))?;
        validate_reserved_shortcut(vk, dictation_mods(dictation), "主快捷键")?;
        shortcuts.insert(
            (vk, dictation_mods(dictation), dictation.press_hold_mode),
            "主快捷键".to_string(),
        );
    }

    for profile in &dictation.shortcut_profiles {
        if profile.id.is_empty() || !ids.insert(profile.id.clone()) {
            return Err("快捷键方案 ID 不能为空且不能重复".to_string());
        }
        if profile.name.is_empty() {
            return Err("快捷键方案名称不能为空".to_string());
        }
        if profile.name.chars().count() > 80 {
            return Err(format!(
                "快捷键方案「{}」名称不能超过 80 个字符",
                profile.name
            ));
        }
        if profile
            .smart_processing_min_chars
            .is_some_and(|value| value > 10_000)
        {
            return Err(format!(
                "快捷键方案「{}」的智能处理最少字符数不能超过 10000",
                profile.name
            ));
        }
        if let Some(template_id) = &profile.smart_template_id {
            if !known_templates.contains(template_id) {
                return Err(format!(
                    "快捷键方案「{}」引用的智能模板不存在",
                    profile.name
                ));
            }
        }
        if profile.key_code.is_empty() {
            if profile.enabled {
                return Err(format!("快捷键方案「{}」尚未设置快捷键", profile.name));
            }
            continue;
        }
        let vk = hotkey::code_to_vk(&profile.key_code)
            .ok_or_else(|| format!("快捷键方案「{}」使用了不支持的按键", profile.name))?;
        let mods = profile.mods();
        validate_reserved_shortcut(vk, mods, &format!("快捷键方案「{}」", profile.name))?;
        if let Some(existing) =
            shortcuts.insert((vk, mods, profile.press_hold_mode()), profile.name.clone())
        {
            return Err(format!(
                "快捷键方案「{}」与{existing}使用了相同快捷键和触发方式",
                profile.name
            ));
        }
    }

    if !subtitle.key_code.trim().is_empty() {
        let subtitle_vk = hotkey::code_to_vk(&subtitle.key_code)
            .ok_or_else(|| "实时字幕使用了不支持的快捷键".to_string())?;
        let subtitle_mods = subtitle_shortcut_mods(subtitle);
        validate_reserved_shortcut(subtitle_vk, subtitle_mods, "实时字幕")?;
        if let Some(owner) = shortcuts.iter().find_map(|(&(vk, mods, _), owner)| {
            (vk == subtitle_vk && mods == subtitle_mods).then_some(owner)
        }) {
            return Err(format!("实时字幕与{owner}使用了相同快捷键"));
        }
    }
    Ok(())
}

fn validate_reserved_shortcut(vk: u16, mods: u8, owner: &str) -> Result<(), String> {
    if vk == 0x77 && mods == (hotkey::MOD_CTRL | hotkey::MOD_SHIFT) {
        return Err(format!(
            "{owner}不能使用当前软件上下文调试快捷键 Ctrl+Shift+F8"
        ));
    }
    Ok(())
}

fn collect_shortcut_bindings(
    dictation: &DictationSettings,
    subtitle: &SubtitleShortcutSettings,
) -> Vec<ShortcutBindingItem> {
    let mut items = Vec::new();
    if !dictation.key_code.trim().is_empty() {
        items.push(ShortcutBindingItem {
            target: ShortcutTarget::DictationMain,
            name: "语音输入 · 主快捷键".to_string(),
            action_label: "跟随当前场景规则".to_string(),
            enabled: true,
            key_code: dictation.key_code.clone(),
            ctrl: dictation.ctrl,
            shift: dictation.shift,
            alt: dictation.alt,
            meta: dictation.meta,
            trigger_mode: if dictation.press_hold_mode {
                ShortcutTriggerMode::PressHold
            } else {
                ShortcutTriggerMode::Toggle
            },
            trigger_mode_editable: true,
        });
    }
    items.extend(
        dictation
            .shortcut_profiles
            .iter()
            .filter(|profile| !profile.key_code.trim().is_empty())
            .map(|profile| ShortcutBindingItem {
                target: ShortcutTarget::DictationProfile {
                    profile_id: profile.id.clone(),
                },
                name: format!("语音输入 · {}", profile.name),
                action_label: processing_mode_label(profile.processing_mode).to_string(),
                enabled: profile.enabled,
                key_code: profile.key_code.clone(),
                ctrl: profile.ctrl,
                shift: profile.shift,
                alt: profile.alt,
                meta: profile.meta,
                trigger_mode: profile.trigger_mode,
                trigger_mode_editable: true,
            }),
    );
    if !subtitle.key_code.trim().is_empty() {
        items.push(ShortcutBindingItem {
            target: ShortcutTarget::Subtitles,
            name: "实时字幕".to_string(),
            action_label: "开启或关闭实时字幕".to_string(),
            enabled: true,
            key_code: subtitle.key_code.clone(),
            ctrl: subtitle.ctrl,
            shift: subtitle.shift,
            alt: subtitle.alt,
            meta: subtitle.meta,
            trigger_mode: ShortcutTriggerMode::Toggle,
            trigger_mode_editable: false,
        });
    }
    items
}

fn processing_mode_label(mode: ShortcutProcessingMode) -> &'static str {
    match mode {
        ShortcutProcessingMode::FollowScene => "跟随场景",
        ShortcutProcessingMode::Raw => "原文输出",
        ShortcutProcessingMode::LocalOnly => "仅本地处理",
        ShortcutProcessingMode::SmartOnly => "仅智能处理",
        ShortcutProcessingMode::SmartAndLocal => "智能处理后再本地处理",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(
        id: &str,
        key_code: &str,
        trigger_mode: ShortcutTriggerMode,
    ) -> DictationShortcutProfile {
        DictationShortcutProfile {
            id: id.into(),
            name: id.into(),
            enabled: true,
            key_code: key_code.into(),
            ctrl: true,
            shift: false,
            alt: false,
            meta: false,
            processing_mode: ShortcutProcessingMode::SmartOnly,
            trigger_mode,
            smart_template_id: None,
            smart_processing_min_chars: Some(0),
            inject_method: None,
        }
    }

    #[test]
    fn catalog_only_contains_bound_items_and_keeps_disabled_profiles() {
        let mut dictation = DictationSettings {
            key_code: String::new(),
            ..Default::default()
        };
        let mut bound = profile("bound", "F9", ShortcutTriggerMode::Toggle);
        bound.enabled = false;
        dictation.shortcut_profiles =
            vec![bound, profile("empty", "", ShortcutTriggerMode::Toggle)];
        let items = collect_shortcut_bindings(&dictation, &SubtitleShortcutSettings::default());
        assert_eq!(items.len(), 1);
        assert!(!items[0].enabled);
    }

    #[test]
    fn clearing_profile_keeps_business_fields_and_disables_it() {
        let mut dictation = DictationSettings {
            shortcut_profiles: vec![profile("smart", "F9", ShortcutTriggerMode::PressHold)],
            ..Default::default()
        };
        let before = dictation.shortcut_profiles[0].processing_mode;
        clear_binding(
            &mut dictation,
            &mut SubtitleShortcutSettings::default(),
            &ShortcutTarget::DictationProfile {
                profile_id: "smart".into(),
            },
        )
        .unwrap();
        let profile = &dictation.shortcut_profiles[0];
        assert!(profile.key_code.is_empty());
        assert!(!profile.enabled);
        assert_eq!(profile.processing_mode, before);
        assert_eq!(profile.trigger_mode, ShortcutTriggerMode::PressHold);
    }

    #[test]
    fn subtitle_conflicts_with_profiles_in_both_trigger_modes() {
        for mode in [ShortcutTriggerMode::Toggle, ShortcutTriggerMode::PressHold] {
            let state = RuntimeState::default();
            let dictation = DictationSettings {
                key_code: String::new(),
                shortcut_profiles: vec![profile("profile", "F9", mode)],
                ..Default::default()
            };
            let subtitle = SubtitleShortcutSettings {
                key_code: "F9".into(),
                ctrl: true,
                ..Default::default()
            };
            assert!(validate_shortcut_settings(&dictation, &subtitle, &state).is_err());
        }
    }

    #[test]
    fn same_combo_with_different_dictation_triggers_is_valid() {
        let state = RuntimeState::default();
        let dictation = DictationSettings {
            key_code: String::new(),
            shortcut_profiles: vec![
                profile("toggle", "F9", ShortcutTriggerMode::Toggle),
                profile("hold", "F9", ShortcutTriggerMode::PressHold),
            ],
            ..Default::default()
        };
        assert!(validate_shortcut_settings(
            &dictation,
            &SubtitleShortcutSettings::default(),
            &state,
        )
        .is_ok());
    }

    #[test]
    fn target_contract_is_tagged_and_uses_profile_id() {
        let value = serde_json::to_value(ShortcutTarget::DictationProfile {
            profile_id: "profile-id".into(),
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({"kind":"dictationProfile","profileId":"profile-id"})
        );
    }

    #[test]
    fn updating_disabled_profile_preserves_disabled_state() {
        let mut item = profile("profile", "F9", ShortcutTriggerMode::Toggle);
        item.enabled = false;
        let mut dictation = DictationSettings {
            key_code: String::new(),
            shortcut_profiles: vec![item],
            ..Default::default()
        };
        apply_binding_update(
            &mut dictation,
            &mut SubtitleShortcutSettings::default(),
            &ShortcutTarget::DictationProfile {
                profile_id: "profile".into(),
            },
            ShortcutBindingInput {
                key_code: "F10".into(),
                ctrl: false,
                shift: true,
                alt: false,
                meta: false,
                trigger_mode: ShortcutTriggerMode::PressHold,
            },
        )
        .unwrap();
        let updated = &dictation.shortcut_profiles[0];
        assert!(!updated.enabled);
        assert_eq!(updated.key_code, "F10");
        assert_eq!(updated.trigger_mode, ShortcutTriggerMode::PressHold);
    }

    #[test]
    fn missing_profile_and_press_hold_subtitles_are_rejected() {
        let mut dictation = DictationSettings {
            key_code: String::new(),
            ..Default::default()
        };
        let binding = ShortcutBindingInput {
            key_code: "F10".into(),
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
            trigger_mode: ShortcutTriggerMode::Toggle,
        };
        assert!(apply_binding_update(
            &mut dictation,
            &mut SubtitleShortcutSettings::default(),
            &ShortcutTarget::DictationProfile {
                profile_id: "missing".into(),
            },
            binding.clone(),
        )
        .unwrap_err()
        .contains("已不存在"));

        let mut subtitle_binding = binding;
        subtitle_binding.trigger_mode = ShortcutTriggerMode::PressHold;
        assert!(apply_binding_update(
            &mut dictation,
            &mut SubtitleShortcutSettings::default(),
            &ShortcutTarget::Subtitles,
            subtitle_binding,
        )
        .unwrap_err()
        .contains("仅支持单击切换"));
    }

    #[test]
    fn subtitle_cannot_use_reserved_debug_shortcut() {
        let state = RuntimeState::default();
        let subtitle = SubtitleShortcutSettings {
            key_code: "F8".into(),
            ctrl: true,
            shift: true,
            ..Default::default()
        };
        assert!(validate_shortcut_settings(
            &DictationSettings {
                key_code: String::new(),
                ..Default::default()
            },
            &subtitle,
            &state,
        )
        .unwrap_err()
        .contains("上下文调试"));
    }
}
