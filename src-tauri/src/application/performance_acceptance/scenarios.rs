use super::*;
use crate::state::RuntimeState;

pub(super) async fn subtitles(app: &AppHandle, recorder: &mut Recorder) -> Result<(), String> {
    for cycle in 1..=5 {
        let started = Instant::now();
        crate::application::subtitles::show_subtitle_preview(app.clone(), Default::default())
            .await?;
        let snapshot = serde_json::to_value(crate::application::subtitles::get_subtitle_runtime(
            app.state(),
        )?)
        .map_err(|e| e.to_string())?;
        if snapshot["previewActive"] != true || !snapshot["sessionId"].is_null() {
            return Err("字幕预览意外启动识别会话或未激活".into());
        }
        recorder.record(
            "preview-active",
            cycle,
            Some(started.elapsed().as_secs_f64() * 1000.0),
        )?;
        tokio::time::sleep(Duration::from_secs(8)).await;
        let started = Instant::now();
        crate::application::subtitles::hide_subtitle_preview(app.clone()).await?;
        if crate::application::subtitles::owns_indicator(&app.state()) {
            return Err("字幕预览停止后仍占用指示窗".into());
        }
        recorder.record(
            "preview-stopped",
            cycle,
            Some(started.elapsed().as_secs_f64() * 1000.0),
        )?;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Ok(())
}

async fn seed_audio(app: &AppHandle, seconds: usize) -> Result<(), String> {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<RuntimeState>()
            .audio_lab_runtime
            .acceptance_seed(seconds)
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(super) async fn audio_lab(app: &AppHandle, recorder: &mut Recorder) -> Result<(), String> {
    use crate::application::audio_lab::{
        audio_lab_audio_path, audio_lab_reprocess, get_audio_lab_runtime,
    };
    use crate::audio_dsp::DspParams;
    let params = DspParams {
        denoise_enabled: false,
        ..Default::default()
    };
    let error = audio_lab_reprocess(app.clone(), params.clone())
        .await
        .err()
        .ok_or("空素材处理未拒绝")?;
    if error != "请先录制音频" {
        return Err(format!("空素材错误异常：{error}"));
    }
    recorder.detail("empty-input-rejected", 0, None, json!({"error":error}))?;
    let mut previous_hash = None;
    let mut previous_preview: Option<String> = None;
    for cycle in 1..=3 {
        recorder.record("audio-loading", cycle, None)?;
        let started = Instant::now();
        seed_audio(app, 1800).await?;
        let snapshot = get_audio_lab_runtime(app.clone()).await?;
        if snapshot.recording || snapshot.duration_ms != 1_800_000 {
            return Err("长素材快照不完整".into());
        }
        recorder.record(
            "audio-recorded",
            cycle,
            Some(started.elapsed().as_secs_f64() * 1000.0),
        )?;
        tokio::time::sleep(Duration::from_secs(4)).await;
        recorder.record("audio-processing", cycle, None)?;
        let started = Instant::now();
        let snapshot = audio_lab_reprocess(app.clone(), params.clone()).await?;
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        if snapshot.duration_ms != 1_800_000
            || snapshot.stats.is_none()
            || snapshot.processed_waveform.is_empty()
        {
            return Err("长素材处理结果不完整".into());
        }
        // 完整内容校验产生额外读取，不纳入处理耗时。
        let hash_app = app.clone();
        let hash = tauri::async_runtime::spawn_blocking(move || {
            hash_app
                .state::<RuntimeState>()
                .audio_lab_runtime
                .acceptance_processed_hash(1800)
        })
        .await
        .map_err(|e| e.to_string())??;
        if previous_hash
            .as_ref()
            .is_some_and(|previous| previous != &hash)
        {
            return Err("重复处理输出不一致".into());
        }
        previous_hash = Some(hash.clone());
        recorder.detail(
            "audio-processed",
            cycle,
            Some(elapsed),
            json!({"seconds":1800,"denoise":false,"outputHash":hash}),
        )?;
        tokio::time::sleep(Duration::from_secs(4)).await;

        // 走下一次录音的正常替换边界，保留一秒素材；不调用测试专用清空或工作集修剪。
        seed_audio(app, 1).await?;
        let short = audio_lab_reprocess(app.clone(), DspParams::default()).await?;
        if short.duration_ms != 1000 || short.stats.is_none() {
            return Err("短素材恢复失败".into());
        }
        let path = audio_lab_audio_path(app.clone(), true).await?;
        let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
        if bytes.get(..4) != Some(b"RIFF") || bytes.len() <= 44 {
            return Err("试听文件无效".into());
        }
        if previous_preview
            .as_ref()
            .is_some_and(|old| std::path::Path::new(old).exists())
        {
            return Err("旧试听文件未释放".into());
        }
        previous_preview = Some(path);
        recorder.record("audio-replaced", cycle, None)?;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    Ok(())
}
