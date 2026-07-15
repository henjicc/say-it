use std::collections::HashSet;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::codecs::png::{CompressionType, FilterType as PngFilterType, PngEncoder};
use image::{ColorType, DynamicImage, ImageEncoder};
use ocr_rs::{DetOptions, OcrEngine, OcrEngineConfig};
use sha2::{Digest, Sha256};

use super::model::{NormalizedRegion, OcrTextBlock};
use super::normalize::normalize_text;

const OCR_TEXT_LIMIT: usize = 2_000;
const MAX_BLOCKS: usize = 120;
const MAX_RECOGNIZED_REGIONS: usize = 96;
const DET_MODEL: &str = "PP-OCRv6_tiny_det.mnn";
const REC_MODEL: &str = "PP-OCRv6_tiny_rec.mnn";
const CHARSET: &str = "ppocr_keys_v6_tiny.txt";
const DET_SHA256: &str = "7FAB7B858F136BC93A760BDCA66AAF25F0FF10ACCABB31E6EF853A897FB9CFEC";
const REC_SHA256: &str = "0A43C3C979A98B905F5E84913209998F510189419B5A5D4152BBB01CE8D17A93";
const CHARSET_SHA256: &str = "C5CBE34EF40C29C4DF07ED012BF96569CB69A2D2A01A07027E9F13CB832BD9CD";

static MODEL_ROOT: OnceLock<PathBuf> = OnceLock::new();
static OCR_ENGINE: OnceLock<Result<Mutex<OcrEngine>, String>> = OnceLock::new();
static MODEL_INIT_MS: OnceLock<u64> = OnceLock::new();
static OCR_WORKER: OnceLock<SyncSender<OcrTask>> = OnceLock::new();

struct OcrTask {
    image: DynamicImage,
    reply: SyncSender<Result<OcrPipelineOutput, String>>,
}

pub(crate) fn configure_model_root(path: PathBuf) {
    let _ = MODEL_ROOT.set(path);
}

#[derive(Debug)]
pub(crate) struct OcrPipelineOutput {
    pub(crate) text: Vec<String>,
    pub(crate) blocks: Vec<OcrTextBlock>,
    pub(crate) elapsed_ms: u64,
    pub(crate) model_init_ms: u64,
    pub(crate) truncated: bool,
}

fn model_root() -> PathBuf {
    MODEL_ROOT.get().cloned().unwrap_or_else(|| {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("resources")
            .join("ocr")
    })
}

fn verify_file(path: &Path, expected: &str) -> Result<(), String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("读取 OCR 模型 {} 失败：{error}", path.display()))?;
    let actual = format!("{:X}", Sha256::digest(bytes));
    if actual != expected {
        return Err(format!("OCR 模型校验失败：{}", path.display()));
    }
    Ok(())
}

fn build_engine() -> Result<Mutex<OcrEngine>, String> {
    let started = Instant::now();
    let root = model_root();
    let det = root.join(DET_MODEL);
    let rec = root.join(REC_MODEL);
    let charset = root.join(CHARSET);
    verify_file(&det, DET_SHA256)?;
    verify_file(&rec, REC_SHA256)?;
    verify_file(&charset, CHARSET_SHA256)?;
    let config = OcrEngineConfig::new()
        .with_threads(3)
        .with_parallel(false)
        .with_min_result_confidence(0.45)
        .with_det_options(
            DetOptions::default()
                .with_max_side_len(1_600)
                .with_box_threshold(0.4)
                .with_score_threshold(0.25),
        );
    let engine = OcrEngine::new(det, rec, charset, Some(config))
        .map_err(|error| format!("初始化 PP-OCRv6 tiny 失败：{error}"))?;
    let _ = MODEL_INIT_MS.set(started.elapsed().as_millis() as u64);
    Ok(Mutex::new(engine))
}

fn engine() -> Result<&'static Mutex<OcrEngine>, String> {
    OCR_ENGINE
        .get_or_init(build_engine)
        .as_ref()
        .map_err(Clone::clone)
}

fn recognize_full_window(image: &DynamicImage) -> Result<Vec<OcrTextBlock>, String> {
    let width = image.width().max(1) as f32;
    let height = image.height().max(1) as f32;
    let guard = engine()?
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let results = guard
        .recognize_limited(image, MAX_RECOGNIZED_REGIONS)
        .map_err(|error| format!("OCR 识别失败：{error}"))?;
    drop(guard);

    let mut blocks = results
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
    sort_blocks_by_reading_order(&mut blocks);
    let mut seen = HashSet::new();
    blocks.retain(|block| seen.insert(block.text.to_lowercase()));
    blocks.truncate(MAX_BLOCKS);
    Ok(blocks)
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

fn run_full_window_inner(image: &DynamicImage) -> Result<OcrPipelineOutput, String> {
    let started = Instant::now();
    let blocks = recognize_full_window(image)?;
    let (text, truncated) = block_text(&blocks);
    Ok(OcrPipelineOutput {
        text,
        blocks,
        elapsed_ms: started.elapsed().as_millis() as u64,
        model_init_ms: *MODEL_INIT_MS.get().unwrap_or(&0),
        truncated,
    })
}

fn worker() -> &'static SyncSender<OcrTask> {
    OCR_WORKER.get_or_init(|| {
        let (sender, receiver) = sync_channel::<OcrTask>(2);
        std::thread::Builder::new()
            .name("active-app-ocr".into())
            .spawn(move || {
                while let Ok(task) = receiver.recv() {
                    let result =
                        catch_unwind(AssertUnwindSafe(|| run_full_window_inner(&task.image)))
                            .unwrap_or_else(|_| Err("OCR 内部处理异常，已跳过本次识别".into()));
                    let _ = task.reply.send(result);
                }
            })
            .expect("failed to start active app OCR worker");
        sender
    })
}

pub(crate) fn run_full_window(
    image: DynamicImage,
    timeout: Duration,
) -> Result<OcrPipelineOutput, String> {
    if timeout.is_zero() {
        return Err("OCR 任务已超时".into());
    }
    let (reply, receiver) = sync_channel(1);
    match worker().try_send(OcrTask { image, reply }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Err("OCR 任务队列已满".into()),
        Err(TrySendError::Disconnected(_)) => return Err("OCR 工作线程不可用".into()),
    }
    match receiver.recv_timeout(timeout) {
        Ok(result) => result,
        Err(RecvTimeoutError::Timeout) => Err("OCR 任务已超时".into()),
        Err(RecvTimeoutError::Disconnected) => Err("OCR 工作线程不可用".into()),
    }
}

pub(crate) fn png_data_url(image: &DynamicImage) -> Result<String, String> {
    let rgba = image.to_rgba8();
    let mut bytes = Vec::new();
    PngEncoder::new_with_quality(&mut bytes, CompressionType::Fast, PngFilterType::Adaptive)
        .write_image(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
            ColorType::Rgba8.into(),
        )
        .map_err(|error| format!("生成调试截图失败：{error}"))?;
    Ok(format!("data:image/png;base64,{}", STANDARD.encode(bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn bundled_models_recognize_fixture_and_reuse_engine() {
        let image = image::open(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests")
                .join("fixtures")
                .join("ocr-window.png"),
        )
        .expect("fixture should load");
        let first_engine = engine().expect("bundled model should initialize") as *const _;
        let first = recognize_full_window(&image).expect("fixture OCR should succeed");
        let second_engine = engine().expect("engine should remain available") as *const _;
        let second = recognize_full_window(&image).expect("repeated fixture OCR should succeed");

        assert_eq!(first_engine, second_engine);
        assert!(first.iter().any(|block| {
            let text = block.text.to_lowercase();
            text.contains("ocr") || text.contains("tauri") || text.contains("测试")
        }));
        assert!(!second.is_empty());
        assert!(first.iter().all(|block| {
            block.bounds.left >= 0.0
                && block.bounds.top >= 0.0
                && block.bounds.right <= 1.0
                && block.bounds.bottom <= 1.0
                && block.bounds.right >= block.bounds.left
                && block.bounds.bottom >= block.bounds.top
        }));
    }
}
