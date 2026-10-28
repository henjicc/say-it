use image::DynamicImage;
use ocr_rs::{DetOptions, MemoryMode, OcrEngine, OcrEngineConfig};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Instant;

use super::{normalize_text, NormalizedRegion, OcrTextBlock};
static MODEL_ROOT: OnceLock<PathBuf> = OnceLock::new();
const MODEL_FILES: &[(&str, u64, &str)] = &[
    (
        "PP-OCRv6_tiny_det.mnn",
        901_896,
        "7fab7b858f136bc93a760bdca66aaf25f0ff10accabb31e6ef853a897fb9cfec",
    ),
    (
        "PP-OCRv6_tiny_rec.mnn",
        2_251_616,
        "0a43c3c979a98b905f5e84913209998f510189419b5a5d4152bbb01ce8d17a93",
    ),
    (
        "ppocr_keys_v6_tiny.txt",
        27_156,
        "c5cbe34ef40c29c4df07ed012bf96569cb69a2d2a01a07027e9f13cb832bd9cd",
    ),
];

pub(crate) fn configure_resources(app: &tauri::AppHandle) -> Result<(), String> {
    use tauri::Manager;
    let root = app
        .path()
        .resource_dir()
        .map_err(|error| format!("定位内置 PP-OCR 资源失败：{error}"))?
        .join("resources/ocr");
    MODEL_ROOT
        .set(root)
        .map_err(|_| "内置 PP-OCR 资源目录已配置".to_string())
}

fn model_root() -> Result<PathBuf, String> {
    select_model_root(MODEL_ROOT.get().map(PathBuf::as_path), cfg!(test))
}

fn select_model_root(configured: Option<&Path>, test_fallback: bool) -> Result<PathBuf, String> {
    if let Some(root) = configured {
        return Ok(root.to_path_buf());
    }
    // cargo test 不运行 Tauri setup，只允许测试回落到仓库资源。
    if test_fallback {
        return Ok(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr"));
    }
    Err("内置 PP-OCR 资源目录尚未初始化".into())
}

fn verify_models(root: &Path) -> Result<(), String> {
    for &(name, size, sha256) in MODEL_FILES {
        let path = root.join(name);
        let bytes = std::fs::read(&path).map_err(|error| {
            format!(
                "内置 PP-OCR 文件缺失或无法读取：{}：{error}；请重新安装应用",
                path.display()
            )
        })?;
        if bytes.len() as u64 != size || format!("{:x}", Sha256::digest(&bytes)) != sha256 {
            return Err(format!(
                "内置 PP-OCR 文件完整性校验失败：{}；请重新安装应用",
                path.display()
            ));
        }
    }
    Ok(())
}

pub(crate) const DET_MAX_SIDE_LEN: u32 = 960;
const MAX_BLOCKS: usize = 120;
const MAX_RECOGNIZED_REGIONS: usize = 96;

pub(crate) struct EngineState {
    pub(crate) engine: OcrEngine,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) init_ms: u64,
}

pub(crate) fn build_engine() -> Result<EngineState, String> {
    build_engine_at(&model_root()?)
}

fn build_engine_at(root: &Path) -> Result<EngineState, String> {
    let started = Instant::now();
    verify_models(root)?;
    let det = root.join(MODEL_FILES[0].0);
    let rec = root.join(MODEL_FILES[1].0);
    let charset = root.join(MODEL_FILES[2].0);
    let config = OcrEngineConfig::new()
        .with_threads(3)
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
    let init_ms = started.elapsed().as_millis() as u64;
    crate::development_debug_log(
        "ppocr",
        format_args!("PP-OCR 模型加载完成：{init_ms} ms；引擎将在本次任务结束后释放"),
    );
    Ok(EngineState { engine, init_ms })
}

pub(crate) fn recognize_full_window(
    engine: &OcrEngine,
    image: &DynamicImage,
) -> Result<Vec<OcrTextBlock>, String> {
    let width = image.width().max(1) as f32;
    let height = image.height().max(1) as f32;
    let results = engine
        .recognize_limited(image, MAX_RECOGNIZED_REGIONS)
        .map_err(|error| format!("OCR 识别失败：{error}"))?;

    Ok(results
        .into_iter()
        .filter_map(|result| {
            let text = normalize_text(&result.text);
            if text.is_empty() {
                return None;
            }
            let rect = result.bbox.rect;
            Some(OcrTextBlock {
                text,
                confidence: if result.confidence.is_finite() {
                    result.confidence
                } else {
                    0.0
                },
                bounds: NormalizedRegion {
                    left: rect.left().max(0) as f32 / width,
                    top: rect.top().max(0) as f32 / height,
                    right: (rect.left().max(0) as f32 + rect.width() as f32) / width,
                    bottom: (rect.top().max(0) as f32 + rect.height() as f32) / height,
                }
                .clamped(),
            })
        })
        .collect())
}

fn finalize_blocks(mut blocks: Vec<OcrTextBlock>) -> Vec<OcrTextBlock> {
    blocks.sort_by(|left, right| {
        left.bounds
            .top
            .total_cmp(&right.bounds.top)
            .then_with(|| left.bounds.left.total_cmp(&right.bounds.left))
    });
    let mut seen = HashSet::new();
    blocks.retain(|block| seen.insert(block.text.to_lowercase()));
    blocks.truncate(MAX_BLOCKS);
    blocks
}

pub(crate) fn recognize_png(image_png: &[u8]) -> Result<Vec<OcrTextBlock>, String> {
    let image = image::load_from_memory(image_png)
        .map_err(|error| format!("解码 PP-OCR 图像失败：{error}"))?;
    let engine = build_engine()?;
    recognize_full_window(&engine.engine, &image).map(finalize_blocks)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_resources_take_priority_even_without_source_or_files() {
        let root = std::env::temp_dir()
            .join(uuid::Uuid::new_v4().to_string())
            .join("resources/ocr");
        assert!(!root.exists());
        for test_fallback in [false, true] {
            assert_eq!(
                select_model_root(Some(&root), test_fallback).unwrap(),
                root
            );
        }
    }

    #[test]
    fn only_tests_can_resolve_resources_without_tauri_setup() {
        assert!(select_model_root(None, false)
            .unwrap_err()
            .contains("尚未初始化"));
        assert_eq!(
            select_model_root(None, true).unwrap(),
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources/ocr")
        );
    }

    #[test]
    fn builtin_resources_match_compiled_checksums_and_recognize_fixture() {
        let root = model_root().unwrap();
        verify_models(&root).unwrap();
        let blocks = recognize_png(include_bytes!("../../tests/fixtures/ocr-window.png")).unwrap();
        assert!(blocks.iter().any(|block| {
            let text = block.text.to_lowercase();
            text.contains("ocr") || text.contains("tauri") || text.contains("测试")
        }));
    }

    #[test]
    fn builtin_resources_reject_missing_and_tampered_files() {
        let root = std::env::temp_dir().join(format!("sayit-builtin-ocr-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        assert!(build_engine_at(&root).err().unwrap().contains("文件缺失"));
        let source = model_root().unwrap();
        for &(name, _, _) in MODEL_FILES {
            std::fs::copy(source.join(name), root.join(name)).unwrap();
        }
        verify_models(&root).unwrap();
        let path = root.join(MODEL_FILES[0].0);
        let mut bytes = std::fs::read(&path).unwrap();
        bytes[0] ^= 1;
        std::fs::write(&path, &bytes).unwrap();
        assert!(build_engine_at(&root)
            .err()
            .unwrap()
            .contains("完整性校验失败"));
        bytes.pop();
        std::fs::write(&path, &bytes).unwrap();
        assert!(build_engine_at(&root)
            .err()
            .unwrap()
            .contains("完整性校验失败"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
