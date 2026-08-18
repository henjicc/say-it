use image::DynamicImage;
use ocr_rs::{DetOptions, MemoryMode, OcrEngine, OcrEngineConfig};
use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Instant;

use super::{normalize_text, NormalizedRegion, OcrTextBlock};
use crate::providers::plugin::{LocalModelSpec, ModelPackManifest};

pub(crate) const DET_MAX_SIDE_LEN: u32 = 960;
const MAX_BLOCKS: usize = 120;
const MAX_RECOGNIZED_REGIONS: usize = 96;

pub(crate) struct EngineState {
    pub(crate) engine: OcrEngine,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub(crate) init_ms: u64,
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
    let path = spec
        .model_dir
        .join(crate::providers::plugin::safe_model_file_path(relative)?);
    crate::providers::model_download::verify_model_file(&path, file)?;
    Ok(path)
}

pub(crate) fn build_engine(spec: &LocalModelSpec) -> Result<EngineState, String> {
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
    crate::development_debug_log("ppocr", "PP-OCR 模型加载：校验并加载本地模型");
    let det = model_param_path(spec, "detModel")?;
    let rec = model_param_path(spec, "recModel")?;
    let charset = model_param_path(spec, "charset")?;
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

pub(crate) fn recognize_png(
    spec: &LocalModelSpec,
    image_png: &[u8],
) -> Result<Vec<OcrTextBlock>, String> {
    let image = image::load_from_memory(image_png)
        .map_err(|error| format!("解码 PP-OCR 图像失败：{error}"))?;
    let engine = build_engine(spec)?;
    recognize_full_window(&engine.engine, &image).map(finalize_blocks)
}

#[cfg(test)]
pub(crate) fn bundled_test_model_spec() -> LocalModelSpec {
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

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    #[test]
    fn macos_mnn_recognizes_packaged_fixture() {
        let blocks = recognize_png(
            &bundled_test_model_spec(),
            include_bytes!("../../tests/fixtures/ocr-window.png"),
        )
        .expect("macOS PP-OCR should recognize fixture");
        assert!(blocks.iter().any(|block| {
            let text = block.text.to_lowercase();
            text.contains("ocr") || text.contains("tauri") || text.contains("测试")
        }));
    }
}
