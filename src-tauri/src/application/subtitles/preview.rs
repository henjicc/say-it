use super::*;

const SCRIPT: [(&str, &str); 4] = [
    ("嗨，很高兴认识你，这是实时字幕的预览效果。", "Hi, nice to meet you — this is a preview of the live captions."),
    ("你可以在这里调整字体、颜色、位置和动画，所见即所得。", "You can adjust the font, color, position and animation here, and see the result instantly."),
    ("开启字幕翻译后，识别到的内容会实时翻译成你选择的语言。", "Once translation is turned on, recognized speech is translated into your chosen language in real time."),
    ("调整满意后，点击开始字幕就可以正式使用啦。", "Once you're happy with the look, just click Start Captions to begin using it."),
];

pub(super) struct Preview {
    prefs: SubtitlePrefs,
    started: Instant,
    cancellation: CancellationToken,
    presented: bool,
    last_frame: Option<(String, String)>,
}

impl Drop for Preview {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

pub(super) fn is_active(state: &RuntimeState) -> bool {
    state
        .subtitle_runtime
        .preview
        .lock()
        .map(|p| p.is_some())
        .unwrap_or(false)
}

// 独立于识别会话：不读取凭据、不占用音频，也不把演示内容发送到 OBS。
#[tauri::command]
pub(crate) async fn show_subtitle_preview(
    app: AppHandle,
    prefs: SubtitlePrefs,
) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let _guard = state.subtitle_runtime.operation.lock().await;
    let phase = state
        .subtitle_runtime
        .session
        .lock()
        .map_err(|_| "字幕状态锁失败")?
        .phase;
    if !matches!(phase, SubtitlePhase::Idle | SubtitlePhase::Failed) {
        return Err("请先停止实时字幕再预览".into());
    }
    stop_locked(&app)?;
    let cancellation = CancellationToken::new();
    *state
        .subtitle_runtime
        .preview
        .lock()
        .map_err(|_| "字幕预览状态锁失败")? = Some(Preview {
        prefs,
        started: Instant::now(),
        cancellation: cancellation.clone(),
        presented: false,
        last_frame: None,
    });
    if let Err(error) = render_frame(&app) {
        stop_locked(&app)?;
        return Err(error);
    }
    publish_state(&app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::select! {
                _ = cancellation.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_millis(26)) => {}
            }
            let state = app.state::<RuntimeState>();
            let _guard = state.subtitle_runtime.operation.lock().await;
            // 关闭后立刻重开时，旧任务不能再向新预览写入一帧。
            if cancellation.is_cancelled() {
                break;
            }
            if let Err(error) = render_frame(&app) {
                dlog!("[subtitles] 预览绘制失败: {error}");
                if let Err(error) = stop_locked(&app) {
                    dlog!("[subtitles] 关闭预览失败: {error}");
                }
                break;
            }
        }
    });
    Ok(())
}

#[tauri::command]
pub(crate) async fn hide_subtitle_preview(app: AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let _guard = state.subtitle_runtime.operation.lock().await;
    stop_locked(&app)
}

pub(super) fn stop_locked(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let preview = state
        .subtitle_runtime
        .preview
        .lock()
        .map_err(|_| "字幕预览状态锁失败")?
        .take();
    if preview.is_none() {
        return Ok(());
    }
    drop(preview);
    publish_state(app);
    if !crate::desktop::indicator::dictation_owns_indicator() {
        crate::desktop::set_indicator_text(app.clone(), String::new(), None)?;
        crate::desktop::set_indicator_translation(app.clone(), String::new())?;
        crate::desktop::set_indicator_state(app.clone(), "hidden".into())?;
    }
    Ok(())
}

pub(super) fn refresh(app: &AppHandle, prefs: Option<SubtitlePrefs>) -> Result<(), String> {
    {
        let state = app.state::<RuntimeState>();
        let mut preview = state
            .subtitle_runtime
            .preview
            .lock()
            .map_err(|_| "字幕预览状态锁失败")?;
        if let Some(preview) = preview.as_mut() {
            if let Some(prefs) = prefs {
                preview.prefs = prefs;
            }
            preview.presented = false;
            preview.last_frame = None;
        }
    }
    render_frame(app)
}

fn render_frame(app: &AppHandle) -> Result<(), String> {
    let (prefs, frame, present) = {
        let state = app.state::<RuntimeState>();
        let mut preview = state
            .subtitle_runtime
            .preview
            .lock()
            .map_err(|_| "字幕预览状态锁失败")?;
        let Some(preview) = preview.as_mut() else {
            return Ok(());
        };
        if crate::desktop::indicator::dictation_owns_indicator() {
            preview.presented = false;
            preview.last_frame = None;
            return Ok(());
        }
        let frame = frame_at(preview.started.elapsed(), &preview.prefs);
        if preview.presented && preview.last_frame.as_ref() == Some(&frame) {
            return Ok(());
        }
        let present = !preview.presented;
        preview.presented = true;
        preview.last_frame = Some(frame.clone());
        (preview.prefs.clone(), frame, present)
    };
    // 桌面路由会反查字幕状态，调用期间不能持有预览锁。
    if present {
        sync_presentation_with_prefs(app, &prefs, false)?;
    }
    crate::desktop::set_indicator_text(app.clone(), frame.0, None)?;
    crate::desktop::set_indicator_translation(app.clone(), frame.1)
}

fn sentence_duration(index: usize, prefs: &SubtitlePrefs) -> u64 {
    let (source, translation) = SCRIPT[index];
    let source_ms = source.chars().count() as u64 * 60;
    let translation_ms = if prefs.translation_enabled() {
        translation.chars().count() as u64 * 26 + 260
    } else {
        0
    };
    source_ms.max(translation_ms)
        + if index == SCRIPT.len() - 1 {
            3_000
        } else {
            900
        }
}

// 由时钟确定整帧，修改样式无需重建计时器，译文也不会被上一句的异步回调覆盖。
fn frame_at(elapsed: Duration, prefs: &SubtitlePrefs) -> (String, String) {
    let cycle: u64 = (0..SCRIPT.len()).map(|i| sentence_duration(i, prefs)).sum();
    let mut ms = (elapsed.as_millis() % cycle as u128) as u64;
    let mut index = 0;
    while ms >= sentence_duration(index, prefs) {
        ms -= sentence_duration(index, prefs);
        index += 1;
    }
    let display = |translation: bool| {
        let text = if translation {
            SCRIPT[index].1
        } else {
            SCRIPT[index].0
        };
        let count = if translation {
            ms.saturating_sub(260) / 26 + 1
        } else {
            ms / 60 + 1
        };
        let current: String = text.chars().take(count as usize).collect();
        if prefs.mode == "replace" {
            return current;
        }
        let start = (index + 1).saturating_sub(prefs.line_count.max(1) as usize);
        SCRIPT[start..index]
            .iter()
            .map(|pair| {
                if translation {
                    pair.1.to_string()
                } else {
                    pair.0.to_string()
                }
            })
            .chain(std::iter::once(current))
            .collect::<Vec<_>>()
            .join("\n")
    };
    if !prefs.translation_enabled() {
        (display(false), String::new())
    } else if prefs.translation_layout == "translationOnly" {
        (display(true), String::new())
    } else {
        (display(false), display(true))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_starts_with_visible_text_and_loops() {
        let prefs = SubtitlePrefs::default();
        assert_eq!(
            frame_at(Duration::ZERO, &prefs),
            ("嗨".into(), String::new())
        );
        let cycle = (0..SCRIPT.len())
            .map(|i| sentence_duration(i, &prefs))
            .sum();
        assert_eq!(
            frame_at(Duration::from_millis(cycle), &prefs),
            frame_at(Duration::ZERO, &prefs)
        );
    }

    #[test]
    fn scroll_tracks_keep_matching_history_and_respect_line_limit() {
        let mut prefs = SubtitlePrefs {
            mode: "scroll".into(),
            line_count: 2,
            translation_model: "fixture-only".into(),
            ..Default::default()
        };
        let elapsed =
            Duration::from_millis(sentence_duration(0, &prefs) + sentence_duration(1, &prefs));
        let (source, translation) = frame_at(elapsed, &prefs);
        assert_eq!(source, format!("{}\n开", SCRIPT[1].0));
        assert_eq!(translation, format!("{}\nO", SCRIPT[1].1));
        prefs.mode = "replace".into();
        assert_eq!(frame_at(elapsed, &prefs), ("开".into(), "O".into()));
        prefs.translation_layout = "translationOnly".into();
        assert_eq!(frame_at(elapsed, &prefs), ("O".into(), String::new()));
    }

    #[test]
    fn dropping_preview_cancels_playback_without_creating_a_session() {
        let runtime = SubtitleRuntime::default();
        let cancellation = CancellationToken::new();
        *runtime.preview.lock().unwrap() = Some(Preview {
            prefs: SubtitlePrefs::default(),
            started: Instant::now(),
            cancellation: cancellation.clone(),
            presented: false,
            last_frame: None,
        });
        drop(runtime.preview.lock().unwrap().take());
        assert!(cancellation.is_cancelled());
        let session = runtime.session.lock().unwrap();
        assert_eq!(session.phase, SubtitlePhase::Idle);
        assert!(session.lease.is_none());
        assert!(session.asr_session_id.is_none());
    }
}
