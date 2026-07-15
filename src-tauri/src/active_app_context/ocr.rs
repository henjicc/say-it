use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use image::codecs::png::PngEncoder;
use image::{ColorType, DynamicImage, GenericImageView, ImageEncoder};
use ocr_rs::{DetOptions, OcrEngine, OcrEngineConfig};
use sha2::{Digest, Sha256};

use super::model::{NormalizedRegion, OcrCaptureMode, OcrTextBlock};
use super::normalize::normalize_text;

const OCR_TEXT_LIMIT: usize = 2_000;
const MIN_ADAPTIVE_BLOCKS: usize = 3;
const MIN_ADAPTIVE_CHARS: usize = 30;
const MAX_SELECTED_BLOCKS: usize = 80;
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
    focus: Option<NormalizedRegion>,
    debug: bool,
    reply: SyncSender<Result<OcrPipelineOutput, String>>,
}

pub(crate) fn configure_model_root(path: PathBuf) {
    let _ = MODEL_ROOT.set(path);
}

#[derive(Debug)]
pub(crate) struct OcrPipelineOutput {
    pub(crate) text: Vec<String>,
    pub(crate) blocks: Vec<OcrTextBlock>,
    pub(crate) full_window_text: Vec<String>,
    pub(crate) mode: OcrCaptureMode,
    pub(crate) region: NormalizedRegion,
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
    let options = DetOptions::default()
        .with_max_side_len(1_600)
        .with_box_threshold(0.4)
        .with_score_threshold(0.25);
    let config = OcrEngineConfig::new()
        .with_threads(3)
        .with_parallel(false)
        .with_min_result_confidence(0.45)
        .with_det_options(options);
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

pub(crate) fn adaptive_region(focus: Option<NormalizedRegion>) -> NormalizedRegion {
    match focus.filter(|value| value.is_valid()) {
        Some(focus) => NormalizedRegion {
            left: focus.left - 0.35,
            top: focus.top - 0.45,
            right: focus.right + 0.35,
            bottom: focus.bottom + 0.15,
        }
        .clamped(),
        None => NormalizedRegion {
            left: 0.05,
            top: 0.10,
            right: 0.95,
            bottom: 0.95,
        },
    }
}

fn region_pixels(region: NormalizedRegion, width: u32, height: u32) -> (u32, u32, u32, u32) {
    let region = region.clamped();
    let left = (region.left * width as f32).floor() as u32;
    let top = (region.top * height as f32).floor() as u32;
    let right = ((region.right * width as f32).ceil() as u32).clamp(left + 1, width);
    let bottom = ((region.bottom * height as f32).ceil() as u32).clamp(top + 1, height);
    (left, top, right, bottom)
}

fn recognize_region(
    image: &DynamicImage,
    region: NormalizedRegion,
    focus: Option<NormalizedRegion>,
) -> Result<Vec<OcrTextBlock>, String> {
    let (width, height) = image.dimensions();
    let (left, top, right, bottom) = region_pixels(region, width, height);
    let cropped = image.crop_imm(left, top, right - left, bottom - top);
    let guard = engine()?.lock().map_err(|_| "OCR 引擎锁失败".to_string())?;
    let results = guard
        .recognize(&cropped)
        .map_err(|error| format!("OCR 识别失败：{error}"))?;
    drop(guard);

    let region_width = region.right - region.left;
    let region_height = region.bottom - region.top;
    let crop_width = cropped.width().max(1) as f32;
    let crop_height = cropped.height().max(1) as f32;
    let mut blocks = results
        .into_iter()
        .filter_map(|result| {
            let text = normalize_text(&result.text);
            if text.is_empty() {
                return None;
            }
            let rect = result.bbox.rect;
            let bounds = NormalizedRegion {
                left: region.left + rect.left().max(0) as f32 / crop_width * region_width,
                top: region.top + rect.top().max(0) as f32 / crop_height * region_height,
                right: region.left
                    + (rect.left().max(0) as f32 + rect.width() as f32) / crop_width * region_width,
                bottom: region.top
                    + (rect.top().max(0) as f32 + rect.height() as f32) / crop_height
                        * region_height,
            }
            .clamped();
            Some(OcrTextBlock {
                text,
                confidence: result.confidence,
                bounds,
            })
        })
        .collect::<Vec<_>>();

    if blocks.len() > MAX_SELECTED_BLOCKS {
        blocks.sort_by(|a, b| {
            block_relevance(b, focus)
                .total_cmp(&block_relevance(a, focus))
                .then_with(|| a.bounds.top.total_cmp(&b.bounds.top))
        });
        blocks.truncate(MAX_SELECTED_BLOCKS);
    }
    blocks.sort_by(|a, b| {
        let line_delta = (a.bounds.top - b.bounds.top).abs();
        if line_delta < 0.015 {
            a.bounds.left.total_cmp(&b.bounds.left)
        } else {
            a.bounds
                .top
                .total_cmp(&b.bounds.top)
                .then_with(|| a.bounds.left.total_cmp(&b.bounds.left))
        }
    });
    let mut seen = HashSet::new();
    blocks.retain(|block| seen.insert(block.text.to_lowercase()));
    Ok(blocks)
}

fn block_relevance(block: &OcrTextBlock, focus: Option<NormalizedRegion>) -> f32 {
    let center_x = (block.bounds.left + block.bounds.right) / 2.0;
    let center_y = (block.bounds.top + block.bounds.bottom) / 2.0;
    let distance = focus
        .map(|focus| {
            let focus_x = (focus.left + focus.right) / 2.0;
            let focus_y = (focus.top + focus.bottom) / 2.0;
            ((center_x - focus_x).powi(2) + (center_y - focus_y).powi(2)).sqrt()
        })
        .unwrap_or(0.0);
    let chrome_penalty = if center_y < 0.10 || center_y > 0.96 {
        0.12
    } else {
        0.0
    };
    block.confidence - distance * 0.2 - chrome_penalty
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

fn insufficient(blocks: &[OcrTextBlock]) -> bool {
    blocks.len() < MIN_ADAPTIVE_BLOCKS
        || blocks
            .iter()
            .map(|block| block.text.chars().count())
            .sum::<usize>()
            < MIN_ADAPTIVE_CHARS
}

fn run_pipeline_inner(
    image: &DynamicImage,
    focus: Option<NormalizedRegion>,
    debug: bool,
) -> Result<OcrPipelineOutput, String> {
    let started = Instant::now();
    let region = adaptive_region(focus);
    let adaptive = recognize_region(image, region, focus)?;
    let needs_full = insufficient(&adaptive);
    let full = if needs_full || debug {
        Some(recognize_region(image, NormalizedRegion::FULL, focus)?)
    } else {
        None
    };
    let (blocks, mode) = if needs_full {
        (
            full.clone().unwrap_or_default(),
            OcrCaptureMode::FallbackFullWindow,
        )
    } else {
        (adaptive, OcrCaptureMode::Adaptive)
    };
    let (text, truncated) = block_text(&blocks);
    let full_window_text = full
        .as_deref()
        .map(block_text)
        .map(|value| value.0)
        .unwrap_or_default();
    Ok(OcrPipelineOutput {
        text,
        blocks,
        full_window_text,
        mode,
        region,
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
                    let result = run_pipeline_inner(&task.image, task.focus, task.debug);
                    let _ = task.reply.send(result);
                }
            })
            .expect("failed to start active app OCR worker");
        sender
    })
}

pub(crate) fn run_pipeline(
    image: DynamicImage,
    focus: Option<NormalizedRegion>,
    debug: bool,
    timeout: Duration,
) -> Result<OcrPipelineOutput, String> {
    if timeout.is_zero() {
        return Err("OCR 任务已超时".into());
    }
    let (reply, receiver) = sync_channel(1);
    match worker().try_send(OcrTask {
        image,
        focus,
        debug,
        reply,
    }) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Err("OCR 任务队列已满".into()),
        Err(TrySendError::Disconnected(_)) => return Err("OCR 工作线程不可用".into()),
    }
    receiver
        .recv_timeout(timeout)
        .map_err(|_| "OCR 任务已超时".to_string())?
}

pub(crate) fn png_data_url(image: &DynamicImage) -> Result<String, String> {
    let rgba = image.to_rgba8();
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
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

    #[test]
    fn focus_region_expands_by_window_percentages() {
        let region = adaptive_region(Some(NormalizedRegion {
            left: 0.50,
            top: 0.70,
            right: 0.55,
            bottom: 0.75,
        }));
        assert_eq!(
            region,
            NormalizedRegion {
                left: 0.15,
                top: 0.25,
                right: 0.90,
                bottom: 0.90,
            }
        );
    }

    #[test]
    fn focus_region_clamps_at_window_edges() {
        let region = adaptive_region(Some(NormalizedRegion {
            left: 0.01,
            top: 0.01,
            right: 0.04,
            bottom: 0.04,
        }));
        assert_eq!(region.left, 0.0);
        assert_eq!(region.top, 0.0);
        assert!((region.right - 0.39).abs() < 0.0001);
        assert!((region.bottom - 0.19).abs() < 0.0001);
    }

    #[test]
    fn region_pixels_scale_equally_across_resolutions() {
        let region = adaptive_region(None);
        assert_eq!(region_pixels(region, 1_920, 1_080), (96, 108, 1_824, 1_026));
        assert_eq!(
            region_pixels(region, 2_560, 1_440),
            (128, 144, 2_432, 1_368)
        );
        assert_eq!(
            region_pixels(region, 3_840, 2_160),
            (192, 216, 3_648, 2_052)
        );
    }

    #[test]
    fn sparse_results_require_full_window_fallback() {
        let blocks = vec![OcrTextBlock {
            text: "短文本".into(),
            confidence: 0.9,
            bounds: NormalizedRegion::FULL,
        }];
        assert!(insufficient(&blocks));
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
        let first = recognize_region(&image, NormalizedRegion::FULL, None)
            .expect("fixture OCR should succeed");
        let second_engine = engine().expect("engine should remain available") as *const _;
        let second = recognize_region(&image, NormalizedRegion::FULL, None)
            .expect("repeated fixture OCR should succeed");

        assert_eq!(first_engine, second_engine);
        assert!(first.iter().any(|block| {
            let text = block.text.to_lowercase();
            text.contains("ocr") || text.contains("tauri") || text.contains("测试")
        }));
        assert!(!second.is_empty());
        assert!(first.iter().all(|block| block.bounds.is_valid()));
    }
}
