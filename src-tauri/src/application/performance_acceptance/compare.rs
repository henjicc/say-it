use super::fixture::{audio, wait_jobs_empty, Fixture, FILE_A, FILE_B, LIVE};
use super::*;
use crate::application::compare::{
    compare_cancel, compare_start, get_compare_runtime, CompareStartRequest,
};
use crate::state::RuntimeState;

async fn wait_settled(app: &AppHandle) -> Result<(), String> {
    wait_jobs_empty(app).await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while get_compare_runtime(app.state()).phase != "idle" {
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| "模型对比未回到闲置".to_string())?;
    if app.state::<RuntimeState>().audio_session.is_busy() {
        return Err("模型对比仍保留音频租约".into());
    }
    if !app
        .state::<RuntimeState>()
        .transcription_runtime
        .snapshots()
        .is_empty()
    {
        return Err("模型对比子任务仍留在转写恢复缓存".into());
    }
    Ok(())
}

pub(super) async fn run(app: &AppHandle, recorder: &mut Recorder) -> Result<(), String> {
    let fixture = Fixture::install(app).await?;
    let long = audio(1800).await?;
    let rounds = recognition_rounds()?;
    for cycle in 1..=rounds {
        for mode in ["success", "cancel", "failure", "success"] {
            let nonce = fixture.configure(app, mode)?;
            recorder.detail("comparison-running", cycle, None, json!({"mode":mode}))?;
            let started = Instant::now();
            compare_start(
                app.clone(),
                CompareStartRequest {
                    source_mode: "upload".into(),
                    file_path: Some(long.path().to_string_lossy().into_owned()),
                    models: vec![FILE_A.into(), FILE_B.into()],
                    device_name: None,
                    params: None,
                },
            )
            .await?;
            let mut cancel_started = None;
            if mode == "cancel" {
                fixture.wait_ready(&nonce).await?;
                tokio::time::sleep(Duration::from_secs(4)).await;
                cancel_started = Some(Instant::now());
                compare_cancel(app.clone())?;
            }
            wait_settled(app).await?;
            let cancel_cleanup_ms =
                cancel_started.map(|instant| instant.elapsed().as_secs_f64() * 1000.0);
            let snapshot = get_compare_runtime(app.state());
            if snapshot.cells.len() != 2 {
                return Err("模型对比结果格数量异常".into());
            }
            for cell in &snapshot.cells {
                if mode == "success" {
                    if cell.status != "done" || cell.text != "本地验收语句。".repeat(2000) {
                        return Err(format!("对比结果不完整：{}", cell.error_message));
                    }
                } else if cell.status != "error"
                    || (mode == "failure"
                        && !cell.error_message.contains("acceptance-provider-failure"))
                {
                    return Err(format!("对比取消或失败状态异常：{}", cell.error_message));
                }
            }
            recorder.detail(
                "comparison-settled",
                cycle,
                Some(started.elapsed().as_secs_f64() * 1000.0),
                json!({"mode":mode,"cancelCleanupMs":cancel_cleanup_ms}),
            )?;
            drop(snapshot);
            tokio::time::sleep(Duration::from_secs(4)).await;
        }
        recorder.record("comparison-idle", cycle, None)?;
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
    // 混合文件模型和实时模型：实际解码并按录音节奏投喂，覆盖实时结果直接提交的路径。
    let short = audio(5).await?;
    fixture.configure(app, "success")?;
    recorder.record("comparison-mixed-running", rounds + 1, None)?;
    compare_start(
        app.clone(),
        CompareStartRequest {
            source_mode: "upload".into(),
            file_path: Some(short.path().to_string_lossy().into_owned()),
            models: vec![FILE_A.into(), LIVE.into()],
            device_name: None,
            params: None,
        },
    )
    .await?;
    wait_settled(app).await?;
    let snapshot = get_compare_runtime(app.state());
    if snapshot.cells.len() != 2
        || snapshot.cells[0].status != "done"
        || snapshot.cells[1].status != "done"
        || snapshot.cells[1].text != "实时验收:160000"
    {
        let cells: Vec<_> = snapshot.cells.iter().map(|cell| json!({
            "index":cell.index,"status":cell.status,"textChars":cell.text.chars().count(),
            "textPrefix":cell.text.chars().take(48).collect::<String>(),"error":cell.error_message
        })).collect();
        return Err(format!("混合模型未完整消费音频：{}", json!(cells)));
    }
    recorder.record("comparison-mixed-settled", rounds + 1, None)?;
    drop(snapshot);
    tokio::time::sleep(Duration::from_secs(5)).await;
    let paths = [long.path().to_owned(), short.path().to_owned()];
    drop(long);
    drop(short);
    if paths.iter().any(|path| path.exists()) {
        return Err("对比验收 WAV 未释放".into());
    }
    Ok(())
}
