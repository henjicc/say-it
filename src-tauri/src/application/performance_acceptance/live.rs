//! 使用真实领域命令与采集 worker，仅在麦克风输入和最终外部文字交付处替换 I/O。
use super::fixture::{wait_jobs_empty, Fixture, FILE_A, LIVE};
use super::*;
use crate::state::RuntimeState;
use std::sync::Mutex;

static OUTPUTS: Mutex<Vec<String>> = Mutex::new(Vec::new());

pub(super) fn capture_output(text: &str) {
    OUTPUTS
        .lock()
        .expect("验收输出锁失败")
        .push(text.to_string());
}

fn take_outputs() -> Vec<String> {
    std::mem::take(&mut *OUTPUTS.lock().expect("验收输出锁失败"))
}

fn configure_input(app: &AppHandle, model: &str) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let mut settings = state.app_settings.lock().map_err(|_| "验收设置锁失败")?;
    settings.dictation_prefs = json!({
        "asrModel":model,"micDeviceId":"__sayit_acceptance__","keepAliveMs":0,
        "cueEnabled":false,"smartProcessingEnabled":false,"localRulesEnabled":false,
        "dictationSilenceDisconnectEnabled":false,"subtitleSilenceDisconnectEnabled":false
    });
    settings.subtitle_prefs = json!({
        "asrModel":LIVE,"source":"mic:__sayit_acceptance__","translationModel":"none","obsOutputEnabled":false
    });
    Ok(())
}

async fn feed(app: &AppHandle) -> Result<(), String> {
    let mic = app.state::<RuntimeState>().backend_mic.clone();
    {
        let guard = mic.lock().map_err(|_| "验收采集锁失败")?;
        if guard.sample_rate != 48_000 || guard.worker.is_none() {
            return Err("验收未启动正式采集 worker".into());
        }
    }
    for _ in 0..100 {
        let samples = (0..960)
            .map(|index| ((index % 120) as f32 / 120.0 - 0.5) * 0.2)
            .collect();
        crate::desktop::backend_mic::push_backend_mic_samples(&mic, samples);
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    Ok(())
}

async fn feed_until_subtitles_fail(app: &AppHandle) -> Result<(), String> {
    let mic = app.state::<RuntimeState>().backend_mic.clone();
    // 字幕会自动重连；持续提供输入，使每次新连接都真正触发失败，直到预算耗尽。
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if snapshot(app, true)?["phase"] == "failed" {
                return Ok::<_, String>(());
            }
            let samples = (0..960)
                .map(|index| ((index % 120) as f32 / 120.0 - 0.5) * 0.2)
                .collect();
            crate::desktop::backend_mic::push_backend_mic_samples(&mic, samples);
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .map_err(|_| "字幕连续失败未在重试预算内结束".to_string())?
}

fn snapshot(app: &AppHandle, subtitles: bool) -> Result<serde_json::Value, String> {
    if subtitles {
        serde_json::to_value(crate::application::subtitles::get_subtitle_runtime(
            app.state(),
        )?)
    } else {
        serde_json::to_value(crate::application::dictation::get_dictation_runtime(
            app.state(),
        )?)
    }
    .map_err(|error| error.to_string())
}

async fn wait_settled(app: &AppHandle, subtitles: bool) -> Result<serde_json::Value, String> {
    wait_jobs_empty(app).await?;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let value = snapshot(app, subtitles)?;
            if matches!(value["phase"].as_str(), Some("idle" | "failed")) {
                return Ok::<_, String>(value);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| "实时领域未结束".to_string())?
}

fn assert_released(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<RuntimeState>();
    let mic = state.backend_mic.lock().map_err(|_| "验收采集锁失败")?;
    if state.audio_session.is_busy()
        || mic.worker.is_some()
        || !mic.raw_txs.is_empty()
        || mic.tx.is_some()
        || !mic.buffer.is_empty()
        || !mic.pending.is_empty()
    {
        return Err("实时领域停止后仍保留采集、音频队列或租约".into());
    }
    Ok(())
}

async fn dictation_case(
    app: &AppHandle,
    recorder: &mut Recorder,
    fixture: &Fixture,
    model: &str,
    mode: &str,
    cycle: usize,
) -> Result<(), String> {
    use crate::application::dictation::{dictation_cancel, dictation_start, dictation_stop};
    configure_input(app, model)?;
    let nonce = fixture.configure(app, mode)?;
    if !take_outputs().is_empty() {
        return Err("上一听写任务出现迟到的文字交付".into());
    }
    let file = model == FILE_A;
    recorder.detail(
        if file {
            "dictation-file-running"
        } else {
            "dictation-running"
        },
        cycle,
        None,
        json!({"mode":mode}),
    )?;
    let started = Instant::now();
    dictation_start(app.clone()).await?;
    let start_ms = started.elapsed().as_secs_f64() * 1000.0;
    feed(app).await?;
    let stop_started = Instant::now();
    if file {
        dictation_stop(app.clone()).await?;
        if mode == "cancel" {
            fixture.wait_ready(&nonce).await?;
            dictation_cancel(app.clone()).await?;
        }
    } else if mode == "cancel" {
        dictation_cancel(app.clone()).await?;
    } else if mode != "failure" {
        dictation_stop(app.clone()).await?;
    }
    let value = wait_settled(app, false).await?;
    assert_released(app)?;
    let outputs = take_outputs();
    if mode == "success" {
        let expected = if file {
            "本地验收语句。".repeat(2000)
        } else {
            "实时验收:64000".into()
        };
        if value["phase"] != "idle" || outputs != vec![expected] {
            return Err(format!(
                "听写文字未完整且仅交付一次：phase={}，outputs={}，prefix={:?}",
                value["phase"],
                outputs.len(),
                outputs
                    .first()
                    .map(|text| text.chars().take(40).collect::<String>())
            ));
        }
    } else if !outputs.is_empty()
        || (mode == "cancel" && value["phase"] != "idle")
        || (mode == "failure"
            && (value["phase"] != "failed"
                || !value["error"]
                    .as_str()
                    .unwrap_or_default()
                    .contains("acceptance-provider-failure")))
    {
        return Err(format!(
            "听写取消/失败状态异常：phase={}，outputs={}",
            value["phase"],
            outputs.len()
        ));
    }
    recorder.detail(
        if file {
            "dictation-file-settled"
        } else {
            "dictation-settled"
        },
        cycle,
        Some(started.elapsed().as_secs_f64() * 1000.0),
        json!({"mode":mode,"startMs":start_ms,"stopToSettledMs":stop_started.elapsed().as_secs_f64()*1000.0}),
    )?;
    tokio::time::sleep(Duration::from_secs(4)).await;
    if !take_outputs().is_empty() {
        return Err("听写结束后重复交付文字".into());
    }
    Ok(())
}

pub(super) async fn dictation(app: &AppHandle, recorder: &mut Recorder) -> Result<(), String> {
    let fixture = Fixture::install(app).await?;
    for cycle in 1..=recognition_rounds()? {
        for mode in ["success", "cancel", "failure", "success"] {
            dictation_case(app, recorder, &fixture, LIVE, mode, cycle).await?;
        }
        recorder.record("dictation-idle", cycle, None)?;
        tokio::time::sleep(Duration::from_secs(7)).await;
    }
    for mode in ["success", "cancel", "failure", "success"] {
        dictation_case(
            app,
            recorder,
            &fixture,
            FILE_A,
            mode,
            recognition_rounds()? + 1,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn subtitles(app: &AppHandle, recorder: &mut Recorder) -> Result<(), String> {
    use crate::application::subtitles::{subtitle_stop, subtitle_toggle};
    let fixture = Fixture::install(app).await?;
    configure_input(app, LIVE)?;
    for cycle in 1..=recognition_rounds()? {
        for mode in ["success", "failure", "success"] {
            fixture.configure(app, mode)?;
            recorder.detail("subtitles-running", cycle, None, json!({"mode":mode}))?;
            let started = Instant::now();
            subtitle_toggle(app.clone()).await?;
            let start_ms = started.elapsed().as_secs_f64() * 1000.0;
            if mode == "failure" {
                feed_until_subtitles_fail(app).await?;
            } else {
                feed(app).await?;
            }
            let stop_started = Instant::now();
            if mode == "success" {
                tokio::time::timeout(Duration::from_secs(10), async {
                    loop {
                        let value = snapshot(app, true)?;
                        if value["originalText"]
                            .as_str()
                            .unwrap_or_default()
                            .contains("实时验收:")
                        {
                            return Ok::<_, String>(());
                        }
                        tokio::time::sleep(Duration::from_millis(25)).await;
                    }
                })
                .await
                .map_err(|_| "字幕没有投影实时识别文字".to_string())??;
                subtitle_stop(app.clone()).await?;
            }
            let value = wait_settled(app, true).await?;
            assert_released(app)?;
            if mode == "success" && value["phase"] != "idle" {
                return Err(format!("字幕停止后未回到闲置：{}", value["phase"]));
            }
            if mode == "failure"
                && (value["phase"] != "failed"
                    || !value["error"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("字幕 ASR 连接反复中断"))
            {
                return Err(format!("字幕失败未传递：{}", value["phase"]));
            }
            if value["obsOutputActive"] != false {
                return Err("字幕验收意外启动 OBS".into());
            }
            recorder.detail(
                "subtitles-settled",
                cycle,
                Some(started.elapsed().as_secs_f64() * 1000.0),
                json!({"mode":mode,"startMs":start_ms,"stopToSettledMs":stop_started.elapsed().as_secs_f64()*1000.0}),
            )?;
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
        recorder.record("subtitles-idle", cycle, None)?;
        tokio::time::sleep(Duration::from_secs(7)).await;
    }
    Ok(())
}
