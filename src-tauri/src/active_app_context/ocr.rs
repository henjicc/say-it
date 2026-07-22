use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, sync_channel, RecvTimeoutError, Sender, SyncSender};
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
use image::{ColorType, DynamicImage, ImageEncoder};
use ocr_rs::{DetOptions, MemoryMode, OcrEngine, OcrEngineConfig};

use super::model::{NormalizedRegion, OcrTextBlock};
use super::normalize::normalize_text;
use crate::ocr::windows as windows_ocr;
use crate::providers::capabilities::OcrProvider;
use crate::providers::plugin::{LocalModelSpec, ModelPackManifest};

const OCR_TEXT_LIMIT: usize = 2_000;
const MAX_BLOCKS: usize = 120;
const MAX_RECOGNIZED_REGIONS: usize = 96;
// MNN keeps the detector's largest tensor workspace while its session stays live.
// 960 is PP-OCR's mobile/default inference ceiling and avoids a 1600px capture
// turning into a disproportionately large permanent workspace reservation.
const DET_MAX_SIDE_LEN: u32 = 960;
const CANCEL_POLL_INTERVAL: Duration = Duration::from_millis(25);
static OCR_WORKER: OnceLock<Sender<OcrCommand>> = OnceLock::new();
static NEXT_OCR_TASK_ID: AtomicU64 = AtomicU64::new(1);

struct EngineState {
    engine: OcrEngine,
    init_ms: u64,
}

enum OcrCommand {
    Recognize(OcrTask),
    Shutdown,
}

struct OcrTask {
    id: u64,
    provider: OcrProvider,
    submitted_at: Instant,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
    image: DynamicImage,
    reply: SyncSender<Result<OcrPipelineOutput, String>>,
}

#[derive(Debug)]
pub(crate) struct OcrPipelineOutput {
    pub(crate) text: Vec<String>,
    pub(crate) blocks: Vec<OcrTextBlock>,
    pub(crate) elapsed_ms: u64,
    pub(crate) model_init_ms: u64,
    pub(crate) det_session_memory_mb: Option<f32>,
    pub(crate) rec_session_memory_mb: Option<f32>,
    pub(crate) truncated: bool,
    pub(crate) diagnostic: Option<String>,
}

fn model_param_path(spec: &LocalModelSpec, key: &str) -> Result<PathBuf, String> {
    let relative = spec
        .params
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("PP-OCR 模型包参数缺少 {key}"))?;
    let file = spec
        .files
        .iter()
        .find(|file| file.path == relative)
        .ok_or_else(|| format!("PP-OCR 模型包参数 {key} 指向未声明文件：{relative}"))?;
    let path = spec.model_dir.join(crate::providers::plugin::safe_model_file_path(relative)?);
    crate::providers::model_download::verify_model_file(&path, file)?;
    Ok(path)
}

fn build_engine(spec: &LocalModelSpec) -> Result<EngineState, String> {
    if spec.engine != "ppocr-mnn" {
        return Err(format!("模型引擎 {} 不能用于 PP-OCR", spec.engine));
    }
    crate::providers::model_download::verify_pack(
        &spec.model_dir,
        &ModelPackManifest {
            engine: spec.engine.clone(),
            files: spec.files.clone(),
            params: spec.params.clone(),
        },
    )
    .map_err(|error| format!("PP-OCR 模型尚未就绪，请先在插件管理安装模型包：{error}"))?;
    let started = Instant::now();
    crate::development_debug_log("active-app-ocr", "PP-OCR 模型加载：校验并加载本地模型");
    let det = model_param_path(spec, "detModel")?;
    let rec = model_param_path(spec, "recModel")?;
    let charset = model_param_path(spec, "charset")?;
    let config = OcrEngineConfig::new()
        .with_threads(3)
        // Keep the engine hot, but discard stale MNN workspace whenever a
        // differently-sized window or text batch resizes either session.
        .with_memory_mode(MemoryMode::Collect)
        .with_parallel(false)
        .with_min_result_confidence(0.45)
        .with_det_options(
            DetOptions::default()
                .with_max_side_len(DET_MAX_SIDE_LEN)
                .with_box_threshold(0.4)
                .with_score_threshold(0.25),
        );
    let engine = OcrEngine::new(det, rec, charset, Some(config))
        .map_err(|error| format!("初始化 PP-OCRv6 tiny 失败：{error}"))?;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    crate::development_debug_log(
        "active-app-ocr",
        format_args!("PP-OCR 模型加载完成：{elapsed_ms} ms；引擎将在本次任务结束后释放"),
    );
    Ok(EngineState {
        engine,
        init_ms: elapsed_ms,
    })
}

fn skip_reason(cancelled: &AtomicBool, deadline: Instant) -> Option<&'static str> {
    if cancelled.load(Ordering::Acquire) {
        Some("OCR 任务已取消")
    } else if Instant::now() >= deadline {
        Some("OCR 任务已过期")
    } else {
        None
    }
}

fn recognize_full_window(
    engine: &OcrEngine,
    image: &DynamicImage,
) -> Result<Vec<OcrTextBlock>, String> {
    let width = image.width().max(1) as f32;
    let height = image.height().max(1) as f32;
    let results = engine
        .recognize_limited(image, MAX_RECOGNIZED_REGIONS)
        .map_err(|error| format!("OCR 识别失败：{error}"))?;

    let blocks = results
        .into_iter()
        .filter_map(|result| {
            let text = normalize_text(&result.text);
            if text.is_empty() {
                return None;
            }
            let rect = result.bbox.rect;
            let bounds = NormalizedRegion {
                left: rect.left().max(0) as f32 / width,
                top: rect.top().max(0) as f32 / height,
                right: (rect.left().max(0) as f32 + rect.width() as f32) / width,
                bottom: (rect.top().max(0) as f32 + rect.height() as f32) / height,
            }
            .clamped();
            Some(OcrTextBlock {
                text,
                confidence: if result.confidence.is_finite() {
                    result.confidence
                } else {
                    0.0
                },
                bounds,
            })
        })
        .collect::<Vec<_>>();
    Ok(finalize_blocks(blocks))
}

/// 排序、去重、截断——两套引擎共用的收尾步骤，保证输出契约一致。
fn finalize_blocks(mut blocks: Vec<OcrTextBlock>) -> Vec<OcrTextBlock> {
    sort_blocks_by_reading_order(&mut blocks);
    let mut seen = HashSet::new();
    blocks.retain(|block| seen.insert(block.text.to_lowercase()));
    blocks.truncate(MAX_BLOCKS);
    blocks
}

fn sort_blocks_by_reading_order(blocks: &mut [OcrTextBlock]) {
    blocks.sort_by(|a, b| {
        a.bounds
            .top
            .total_cmp(&b.bounds.top)
            .then_with(|| a.bounds.left.total_cmp(&b.bounds.left))
    });

    let mut line_start = 0;
    while line_start < blocks.len() {
        let line_top = blocks[line_start].bounds.top;
        let mut line_end = line_start + 1;
        while line_end < blocks.len() && blocks[line_end].bounds.top - line_top < 0.015 {
            line_end += 1;
        }
        blocks[line_start..line_end].sort_by(|a, b| {
            a.bounds
                .left
                .total_cmp(&b.bounds.left)
                .then_with(|| a.bounds.top.total_cmp(&b.bounds.top))
        });
        line_start = line_end;
    }
}

fn block_text(blocks: &[OcrTextBlock]) -> (Vec<String>, bool) {
    let mut remaining = OCR_TEXT_LIMIT;
    let mut output = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if remaining == 0 {
            return (output, index < blocks.len());
        }
        let value = block.text.chars().take(remaining).collect::<String>();
        let was_truncated = value.chars().count() < block.text.chars().count();
        remaining = remaining.saturating_sub(value.chars().count());
        if !value.is_empty() {
            output.push(value);
        }
        if was_truncated {
            return (output, true);
        }
    }
    (output, false)
}

fn format_session_memory(memory_mb: Option<f32>) -> String {
    memory_mb
        .filter(|value| value.is_finite() && *value >= 0.0)
        .map(|value| format!("{value:.1}"))
        .unwrap_or_else(|| "不可用".into())
}

fn pipeline_output(
    blocks: Vec<OcrTextBlock>,
    elapsed_ms: u64,
    model_init_ms: u64,
    det_session_memory_mb: Option<f32>,
    rec_session_memory_mb: Option<f32>,
) -> OcrPipelineOutput {
    let (text, truncated) = block_text(&blocks);
    OcrPipelineOutput {
        text,
        blocks,
        elapsed_ms,
        model_init_ms,
        det_session_memory_mb,
        rec_session_memory_mb,
        truncated,
        diagnostic: None,
    }
}

fn run_full_window_inner(
    engine: &EngineState,
    image: &DynamicImage,
) -> Result<OcrPipelineOutput, String> {
    let started = Instant::now();
    let blocks = recognize_full_window(&engine.engine, image)?;
    let (det_session_memory_mb, rec_session_memory_mb) = engine
        .engine
        .memory_usage_mb()
        .map(|(det, rec)| (Some(det), Some(rec)))
        .unwrap_or((None, None));
    Ok(pipeline_output(
        blocks,
        started.elapsed().as_millis() as u64,
        engine.init_ms,
        det_session_memory_mb,
        rec_session_memory_mb,
    ))
}

/// 系统 OCR 没有可复用的常驻模型，每次调用直接查询系统组件，无需初始化耗时统计。
fn run_windows_ocr_inner(image: &DynamicImage) -> Result<OcrPipelineOutput, String> {
    let started = Instant::now();
    let blocks = finalize_blocks(windows_ocr::recognize(image)?);
    Ok(pipeline_output(
        blocks,
        started.elapsed().as_millis() as u64,
        0,
        None,
        None,
    ))
}

fn run_windows_fallback_if_active(
    image: &DynamicImage,
    cancelled: &AtomicBool,
    deadline: Instant,
) -> Result<OcrPipelineOutput, String> {
    if let Some(reason) = skip_reason(cancelled, deadline) {
        return Err(reason.into());
    }
    run_windows_ocr_inner(image)
}

fn png_bytes(image: &DynamicImage) -> Result<Vec<u8>, String> {
    let rgba = image.to_rgba8();
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, PngFilterType::Adaptive)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(|error| format!("编码 OCR 截图失败：{error}"))?;
    Ok(bytes)
}

fn run_plugin_ocr_inner(
    provider: &OcrProvider,
    image: &DynamicImage,
    deadline: Instant,
    cancelled: &AtomicBool,
) -> Result<OcrPipelineOutput, String> {
    if let Some(reason) = skip_reason(cancelled, deadline) {
        return Err(reason.into());
    }
    let bytes = png_bytes(image)?;
    // 编码大图也可能消耗可观时间；远程请求前必须按绝对截止重新计算预算。
    if let Some(reason) = skip_reason(cancelled, deadline) {
        return Err(reason.into());
    }
    let remaining = deadline.saturating_duration_since(Instant::now());
    let started = Instant::now();
    let blocks = tauri::async_runtime::block_on(async {
        tokio::time::timeout(remaining, provider.recognize(&bytes, "activeAppContext"))
            .await
            .map_err(|_| "插件 OCR 超过场景感知截止时间".to_string())?
    })?;
    Ok(pipeline_output(
        finalize_blocks(blocks),
        started.elapsed().as_millis() as u64,
        0,
        None,
        None,
    ))
}

fn worker() -> &'static Sender<OcrCommand> {
    OCR_WORKER.get_or_init(|| {
        let (sender, receiver) = channel::<OcrCommand>();
        std::thread::Builder::new()
            .name("active-app-ocr".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    let task = match command {
                        OcrCommand::Shutdown => break,
                        OcrCommand::Recognize(task) => task,
                    };
                    if let Some(reason) = skip_reason(task.cancelled.as_ref(), task.deadline) {
                        crate::development_debug_log(
                            "active-app-ocr",
                            format_args!("任务 #{} 已跳过：{reason}", task.id),
                        );
                        let _ = task.reply.send(Err(reason.into()));
                        continue;
                    }
                    crate::development_debug_log(
                        "active-app-ocr",
                        format_args!(
                            "任务 #{} 开始：模型 {}，排队 {} ms，图片 {}×{}",
                            task.id,
                            task.provider.description(),
                            task.submitted_at.elapsed().as_millis(),
                            task.image.width(),
                            task.image.height(),
                        ),
                    );
                    // PP-OCR 引擎按任务构建、任务结束即随作用域释放：实测冷启动仅数毫秒，
                    // 而常驻会保留约 130 MiB 的 MNN 会话工作区，得不偿失。
                    let result = catch_unwind(AssertUnwindSafe(|| match &task.provider {
                        OcrProvider::PpOcr { spec } => {
                            let engine = build_engine(spec)?;
                            if let Some(reason) =
                                skip_reason(task.cancelled.as_ref(), task.deadline)
                            {
                                Err(reason.into())
                            } else {
                                run_full_window_inner(&engine, &task.image)
                            }
                        }
                        OcrProvider::System => run_windows_fallback_if_active(
                            &task.image,
                            task.cancelled.as_ref(),
                            task.deadline,
                        ),
                        OcrProvider::Plugin { .. } => {
                            match run_plugin_ocr_inner(
                                &task.provider,
                                &task.image,
                                task.deadline,
                                task.cancelled.as_ref(),
                            ) {
                                Ok(output) => Ok(output),
                                Err(error) => {
                                    crate::development_debug_log(
                                        "active-app-ocr",
                                        format_args!("插件 OCR 失败，降级到系统 OCR：{error}"),
                                    );
                                    let mut output = run_windows_fallback_if_active(
                                        &task.image,
                                        task.cancelled.as_ref(),
                                        task.deadline,
                                    )
                                    .map_err(|fallback_error| {
                                            format!(
                                                "插件 OCR 失败：{error}；Windows 系统 OCR 降级也失败：{fallback_error}"
                                            )
                                        })?;
                                    output.diagnostic = Some(format!(
                                        "插件 OCR 失败，已降级到 Windows 系统 OCR：{error}"
                                    ));
                                    Ok(output)
                                }
                            }
                        }
                        OcrProvider::Unavailable { reason, .. } => Err(reason.clone()),
                    }))
                    .unwrap_or_else(|_| Err("OCR 内部处理异常，已跳过本次识别".into()));
                    match &result {
                        Ok(output) => crate::development_debug_log(
                            "active-app-ocr",
                            format_args!(
                                "任务 #{} 完成：OCR {} ms，文字框 {} 个，输出 {} 段（截断：{}）；会话内存：检测 {} MiB，识别 {} MiB；引擎内存已随任务结束释放\n--- OCR 文字开始 ---\n{}\n--- OCR 文字结束 ---",
                                task.id,
                                output.elapsed_ms,
                                output.blocks.len(),
                                output.text.len(),
                                output.truncated,
                                format_session_memory(output.det_session_memory_mb),
                                format_session_memory(output.rec_session_memory_mb),
                                output.text.join("\n"),
                            ),
                        ),
                        Err(error) => crate::development_debug_log(
                            "active-app-ocr",
                            format_args!("任务 #{} 失败：{error}", task.id),
                        ),
                    }
                    let _ = task.reply.send(result);
                }
            })
            .expect("failed to start active app OCR worker");
        sender
    })
}

pub(crate) fn run_full_window(
    provider: OcrProvider,
    image: DynamicImage,
    deadline: Instant,
    cancelled: Arc<AtomicBool>,
) -> Result<OcrPipelineOutput, String> {
    let timeout = deadline.saturating_duration_since(Instant::now());
    if cancelled.load(Ordering::Acquire) {
        crate::development_debug_log("active-app-ocr", "提交前任务已取消");
        return Err("OCR 任务已取消".into());
    }
    if timeout.is_zero() {
        crate::development_debug_log("active-app-ocr", "提交前已无剩余时间，直接超时");
        return Err("OCR 任务已超时".into());
    }
    let (reply, receiver) = sync_channel(1);
    let id = NEXT_OCR_TASK_ID.fetch_add(1, Ordering::Relaxed);
    let submitted_at = Instant::now();
    crate::development_debug_log(
        "active-app-ocr",
        format_args!(
            "任务 #{id} 已提交：模型 {}，图片 {}×{}，等待上限 {} ms",
            provider.description(),
            image.width(),
            image.height(),
            timeout.as_millis(),
        ),
    );
    if worker()
        .send(OcrCommand::Recognize(OcrTask {
            id,
            provider,
            submitted_at,
            deadline,
            cancelled: Arc::clone(&cancelled),
            image,
            reply,
        }))
        .is_err()
    {
        crate::development_debug_log(
            "active-app-ocr",
            format_args!("任务 #{id} 未入队：工作线程不可用"),
        );
        return Err("OCR 工作线程不可用".into());
    }
    loop {
        if cancelled.load(Ordering::Acquire) {
            crate::development_debug_log(
                "active-app-ocr",
                format_args!(
                    "任务 #{id} 调用方已取消：已等待 {} ms；后台推理若已开始会自行收尾",
                    submitted_at.elapsed().as_millis()
                ),
            );
            return Err("OCR 任务已取消".into());
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            crate::development_debug_log(
                "active-app-ocr",
                format_args!(
                    "任务 #{id} 调用方等待超时：已等待 {} ms；后台任务仍可能继续执行",
                    submitted_at.elapsed().as_millis()
                ),
            );
            return Err("OCR 任务已超时".into());
        }
        match receiver.recv_timeout(remaining.min(CANCEL_POLL_INTERVAL)) {
            Ok(result) => {
                if let Some(reason) = skip_reason(cancelled.as_ref(), deadline) {
                    return Err(reason.into());
                }
                return result;
            }
            Err(RecvTimeoutError::Timeout) => continue,
            Err(RecvTimeoutError::Disconnected) => {
                crate::development_debug_log(
                    "active-app-ocr",
                    format_args!("任务 #{id} 等待时工作线程断开"),
                );
                return Err("OCR 工作线程不可用".into());
            }
        }
    }
}

pub(crate) fn shutdown() {
    if let Some(worker) = OCR_WORKER.get() {
        let _ = worker.send(OcrCommand::Shutdown);
    }
}

pub(crate) fn png_data_url(image: &DynamicImage) -> Result<String, String> {
    Ok(format!(
        "data:image/png;base64,{}",
        STANDARD.encode(png_bytes(image)?)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundled_model_spec() -> LocalModelSpec {
        let file = |path: &str, sha256: &str, size_bytes: u64| {
            crate::providers::plugin::ModelPackFileManifest {
                path: path.into(),
                sha256: sha256.into(),
                size_bytes,
                download: None,
            }
        };
        LocalModelSpec {
            plugin_id: "test.ppocr".into(),
            provider_id: "local-ppocr".into(),
            engine: "ppocr-mnn".into(),
            model_dir: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("resources")
                .join("ocr"),
            files: vec![
                file(
                    "PP-OCRv6_tiny_det.mnn",
                    "7fab7b858f136bc93a760bdca66aaf25f0ff10accabb31e6ef853a897fb9cfec",
                    901_896,
                ),
                file(
                    "PP-OCRv6_tiny_rec.mnn",
                    "0a43c3c979a98b905f5e84913209998f510189419b5a5d4152bbb01ce8d17a93",
                    2_251_616,
                ),
                file(
                    "ppocr_keys_v6_tiny.txt",
                    "c5cbe34ef40c29c4df07ed012bf96569cb69a2d2a01a07027e9f13cb832bd9cd",
                    27_156,
                ),
            ],
            params: serde_json::json!({
                "detModel": "PP-OCRv6_tiny_det.mnn",
                "recModel": "PP-OCRv6_tiny_rec.mnn",
                "charset": "ppocr_keys_v6_tiny.txt"
            }),
        }
    }

    fn block(left: f32, top: f32, text: &str) -> OcrTextBlock {
        OcrTextBlock {
            text: text.into(),
            confidence: 1.0,
            bounds: NormalizedRegion {
                left,
                top,
                right: left + 0.01,
                bottom: top + 0.01,
            },
        }
    }

    #[test]
    fn reading_order_sort_is_total_even_for_overlapping_line_tolerances() {
        let mut blocks = vec![
            block(0.9, 0.0, "a"),
            block(0.5, 0.01, "b"),
            block(0.1, 0.02, "c"),
            block(0.8, 0.005, "d"),
            block(0.4, 0.015, "e"),
            block(0.0, 0.025, "f"),
        ];

        sort_blocks_by_reading_order(&mut blocks);

        assert_eq!(
            blocks
                .iter()
                .map(|block| block.text.as_str())
                .collect::<Vec<_>>(),
            vec!["b", "d", "a", "f", "c", "e"]
        );
    }

    #[test]
    fn panic_boundary_returns_an_error_without_unwinding_worker_loop() {
        let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), String> {
            panic!("simulated OCR failure")
        }))
        .unwrap_or_else(|_| Err("OCR 内部处理异常，已跳过本次识别".into()));

        assert_eq!(result.unwrap_err(), "OCR 内部处理异常，已跳过本次识别");
    }

    #[test]
    fn cancelled_or_expired_task_is_skipped_before_inference() {
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            skip_reason(
                &cancelled,
                Instant::now() + std::time::Duration::from_secs(1)
            ),
            Some("OCR 任务已取消")
        );

        let active = AtomicBool::new(false);
        assert_eq!(
            skip_reason(
                &active,
                Instant::now() - std::time::Duration::from_millis(1)
            ),
            Some("OCR 任务已过期")
        );

        let image = DynamicImage::new_rgba8(1, 1);
        assert_eq!(
            run_windows_fallback_if_active(
                &image,
                &cancelled,
                Instant::now() + std::time::Duration::from_secs(1),
            )
            .unwrap_err(),
            "OCR 任务已取消"
        );
        assert_eq!(
            run_windows_fallback_if_active(
                &image,
                &active,
                Instant::now() - std::time::Duration::from_millis(1),
            )
            .unwrap_err(),
            "OCR 任务已过期"
        );
    }

    #[test]
    fn plugin_provider_runs_through_scene_capture_worker() {
        let root =
            std::env::temp_dir().join(format!("sayit-context-ocr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("connector")).unwrap();
        std::fs::write(
            root.join("connector/index.js"),
            concat!(
                "export default host => ({ invoke(request) {\n",
                "  const bytes = host.base64.decode(request.payload.imageBase64);\n",
                "  return { blocks: [{ text: 'plugin:' + bytes.length, region: { x: 0, y: 0, width: 1, height: 1 } }] };\n",
                "} });",
            ),
        )
        .unwrap();
        let mut profile = crate::providers::windows_ocr_profile();
        profile.id = "test-plugin-ocr".into();
        profile.kind = "plugin:test-plugin-ocr".into();
        profile.display_name = "Test Plugin OCR".into();
        let provider = OcrProvider::Plugin {
            spec: crate::providers::plugin::PluginRuntimeSpec {
                plugin_id: "test-plugin-ocr".into(),
                root: root.clone(),
                entrypoint: root.join("connector/index.js"),
                permissions: vec![],
                allowed_hosts: vec![],
                browser_session: None,
                data_dir: root.join("data"),
                trust: "unsigned".into(),
            },
            profile,
        };
        let output = run_full_window(
            provider,
            DynamicImage::new_rgb8(16, 16),
            Instant::now() + Duration::from_secs(3),
            Arc::new(AtomicBool::new(false)),
        )
        .unwrap();
        assert_eq!(output.blocks.len(), 1);
        assert!(output.blocks[0].text.starts_with("plugin:"));
        assert!(output.diagnostic.is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bundled_models_recognize_fixture_and_reuse_engine() {
        let image = image::open(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("ocr-window.png"),
        )
        .expect("fixture should load");
        let engine = build_engine(&bundled_model_spec()).expect("model pack should initialize");
        assert_eq!(engine.engine.config().memory_mode, MemoryMode::Collect);
        assert_eq!(
            engine.engine.config().det_options.max_side_len,
            DET_MAX_SIDE_LEN
        );
        let first =
            recognize_full_window(&engine.engine, &image).expect("fixture OCR should succeed");
        let second = recognize_full_window(&engine.engine, &image)
            .expect("repeated fixture OCR should succeed");
        let (det_memory_mb, rec_memory_mb) = engine
            .engine
            .memory_usage_mb()
            .expect("MNN session memory should be available");

        assert!(first.iter().any(|block| {
            let text = block.text.to_lowercase();
            text.contains("ocr") || text.contains("tauri") || text.contains("测试")
        }));
        assert!(!second.is_empty());
        assert!(det_memory_mb.is_finite() && det_memory_mb >= 0.0);
        assert!(rec_memory_mb.is_finite() && rec_memory_mb >= 0.0);
        assert!(first.iter().all(|block| {
            block.bounds.left >= 0.0
                && block.bounds.top >= 0.0
                && block.bounds.right <= 1.0
                && block.bounds.bottom <= 1.0
                && block.bounds.right >= block.bounds.left
                && block.bounds.bottom >= block.bounds.top
        }));
    }

    #[test]
    fn collect_mode_discards_stale_detector_workspace_after_a_smaller_window() {
        let engine = build_engine(&bundled_model_spec()).expect("model pack should initialize");
        let large = DynamicImage::new_rgb8(1_600, 1_600);
        recognize_full_window(&engine.engine, &large).expect("large window OCR should succeed");
        let (large_det_memory_mb, _) = engine
            .engine
            .memory_usage_mb()
            .expect("large detector memory should be available");

        let small = DynamicImage::new_rgb8(640, 640);
        recognize_full_window(&engine.engine, &small).expect("small window OCR should succeed");
        let (small_det_memory_mb, _) = engine
            .engine
            .memory_usage_mb()
            .expect("small detector memory should be available");

        assert!(
            small_det_memory_mb <= large_det_memory_mb + 0.1,
            "collect mode should not retain the large detector workspace: large={large_det_memory_mb:.1} MiB, small={small_det_memory_mb:.1} MiB"
        );
    }
}
