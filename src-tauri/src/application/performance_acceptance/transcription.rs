use super::fixture::{audio, wait_jobs_empty, Fixture, FILE_A};
use super::*;
use crate::providers::capabilities::TranscriptionParams;
use crate::state::RuntimeState;

pub(super) async fn run(app: &AppHandle, recorder: &mut Recorder) -> Result<(), String> {
    let fixture = Fixture::install(app).await?;
    let wav = audio(1800).await?;
    let path = wav.path().to_string_lossy().into_owned();
    for cycle in 1..=recognition_rounds()? {
        for mode in ["success", "cancel", "failure", "success"] {
            let nonce = fixture.configure(app, mode)?;
            recorder.detail("transcription-running", cycle, None, json!({"mode":mode}))?;
            let started = Instant::now();
            let job = crate::commands::transcription::transcription_start_inner(
                app.clone(),
                &app.state::<RuntimeState>(),
                path.clone(),
                Some(TranscriptionParams {
                    model: FILE_A.into(),
                    ..Default::default()
                }),
                "transcribe",
            )
            .await?;
            let mut cancel_started = None;
            if mode == "cancel" {
                fixture.wait_ready(&nonce).await?;
                tokio::time::sleep(Duration::from_secs(4)).await;
                cancel_started = Some(Instant::now());
                crate::commands::transcription::transcription_cancel_inner(
                    app,
                    &app.state::<RuntimeState>(),
                    &job.job_id,
                )?;
            }
            wait_jobs_empty(app).await?;
            let cancel_cleanup_ms =
                cancel_started.map(|instant| instant.elapsed().as_secs_f64() * 1000.0);
            let snapshots = app
                .state::<RuntimeState>()
                .transcription_runtime
                .snapshots();
            let result = snapshots
                .iter()
                .find(|snapshot| snapshot.job_id == job.job_id)
                .ok_or("转写结果未保留")?;
            if result.active || snapshots.len() != 1 {
                return Err("转写任务或历史结果未收敛".into());
            }
            if mode == "success" {
                if result.stage != "completed"
                    || result.payload["result"]["durationMs"] != 1_800_000
                    || result.payload["result"]["transcripts"][0]["sentences"]
                        .as_array()
                        .map(Vec::len)
                        != Some(2000)
                    || result.payload["result"]["transcripts"][0]["text"]
                        != "本地验收语句。".repeat(2000)
                {
                    return Err(format!(
                        "转写结果不完整：{}",
                        result
                            .payload
                            .get("message")
                            .unwrap_or(&serde_json::Value::Null)
                    ));
                }
            } else if result.stage != "error"
                || (mode == "failure"
                    && !result.payload["message"]
                        .as_str()
                        .unwrap_or_default()
                        .contains("acceptance-provider-failure"))
            {
                return Err("失败或取消未正确投影".into());
            }
            recorder.detail(
                "transcription-settled",
                cycle,
                Some(started.elapsed().as_secs_f64() * 1000.0),
                json!({"mode":mode,"stage":result.stage,"retainedJobs":snapshots.len(),"cancelCleanupMs":cancel_cleanup_ms}),
            )?;
            drop(snapshots);
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
        recorder.record("transcription-idle", cycle, None)?;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    drop(wav);
    if std::path::Path::new(&path).exists() {
        return Err("验收 WAV 未释放".into());
    }
    Ok(())
}
