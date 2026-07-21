use super::{alibabacloud, ProviderProfile, RequestCustomization};
pub use alibabacloud::{
    HotwordEntry, TranscriptionParams, TranscriptionResult, TranscriptionTaskStatus,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Duration;

use super::plugin::LocalModelSpec;
use super::plugin::PluginRuntimeSpec;
use super::plugin_runtime;
use crate::ocr::{NormalizedRegion, OcrTextBlock};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilityError {
    pub provider_id: String,
    pub capability: &'static str,
    pub message: String,
}
impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}
fn unsupported(profile: &ProviderProfile, capability: &'static str) -> CapabilityError {
    CapabilityError {
        provider_id: profile.id.clone(),
        capability,
        message: format!("供应商 {} 不支持 {capability} 能力", profile.display_name),
    }
}
fn api_key(profile: &ProviderProfile) -> Result<String, String> {
    let key = profile
        .config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if key.is_empty() {
        Err("请先在设置中填写阿里云百炼 API Key".into())
    } else {
        Ok(key.into())
    }
}

#[derive(Clone)]
pub enum FileRecognitionProvider {
    AlibabaCloud {
        api_key: String,
        vocabulary_ids: HashMap<String, String>,
        customization: RequestCustomization,
    },
    Plugin {
        spec: PluginRuntimeSpec,
        profile: ProviderProfile,
        customization: RequestCustomization,
    },
    Local {
        spec: LocalModelSpec,
    },
}
#[cfg(test)]
pub fn file_recognition_for(
    profile: &ProviderProfile,
) -> Result<FileRecognitionProvider, CapabilityError> {
    file_recognition_for_with_plugin(profile, None)
}
#[cfg(test)]
pub fn file_recognition_for_with_plugin(
    profile: &ProviderProfile,
    plugin: Option<PluginRuntimeSpec>,
) -> Result<FileRecognitionProvider, CapabilityError> {
    file_recognition_for_with_extensions(profile, plugin, None, RequestCustomization::default())
}
pub fn file_recognition_for_with_extensions(
    profile: &ProviderProfile,
    plugin: Option<PluginRuntimeSpec>,
    local: Option<LocalModelSpec>,
    customization: RequestCustomization,
) -> Result<FileRecognitionProvider, CapabilityError> {
    if let Some(spec) = local {
        return Ok(FileRecognitionProvider::Local { spec });
    }
    if let Some(spec) = plugin {
        return Ok(FileRecognitionProvider::Plugin {
            spec,
            profile: profile.clone(),
            customization,
        });
    }
    match profile.kind.as_str() {
        "alibabacloud-funasr" => Ok(FileRecognitionProvider::AlibabaCloud {
            api_key: api_key(profile).map_err(|message| CapabilityError {
                provider_id: profile.id.clone(),
                capability: "fileRecognition",
                message,
            })?,
            vocabulary_ids: profile
                .config
                .get("vocabularyIds")
                .and_then(|v| serde_json::from_value(v.clone()).ok())
                .unwrap_or_default(),
            customization,
        }),
        _ => Err(unsupported(profile, "fileRecognition")),
    }
}

impl FileRecognitionProvider {
    pub fn uses_async_task(&self, model: &str) -> bool {
        match self {
            Self::AlibabaCloud { .. } => alibabacloud::uses_async_transcription_task(model),
            Self::Plugin { .. } => false,
            Self::Local { .. } => false,
        }
    }
    pub async fn recognize_short(
        &self,
        path: &str,
        params: &TranscriptionParams,
        cancel: Option<Arc<AtomicBool>>,
    ) -> Result<TranscriptionResult, String> {
        match self {
            Self::AlibabaCloud {
                api_key,
                customization,
                ..
            } => alibabacloud::recognize_short_audio(api_key, path, params, customization).await,
            Self::Plugin {
                spec,
                profile,
                customization,
            } => {
                let mut payload = serde_json::Map::new();
                payload.insert("filePath".into(), serde_json::json!(path));
                payload.insert(
                    "params".into(),
                    serde_json::to_value(params).map_err(|error| error.to_string())?,
                );
                customization.write_into(&mut payload);
                let value = plugin_runtime::invoke_cancellable(
                    spec,
                    profile,
                    "transcribeFile",
                    Value::Object(payload),
                    Duration::from_secs(30 * 60),
                    cancel,
                    |_| {},
                )
                .await?;
                serde_json::from_value(value)
                    .map_err(|error| format!("插件文件识别结果格式错误：{error}"))
            }
            Self::Local { spec } => {
                if cancel
                    .as_ref()
                    .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
                {
                    return Err("录音识别已取消".into());
                }
                let spec = spec.clone();
                let path = path.to_string();
                let result = tauri::async_runtime::spawn_blocking(move || {
                    let samples = crate::audio_prep::decode_to_mono_16k(&path)?;
                    let duration_ms = (samples.len() as u64).saturating_mul(1_000) / 16_000;
                    let segments = super::local_asr::recognize_file_segments(&spec, &samples)?;
                    // VAD 句段边界即字幕时间轴，逐句透传，文稿对齐与字幕转写才能用上本地模型。
                    let text = segments
                        .iter()
                        .map(|item| item.text.as_str())
                        .collect::<Vec<_>>()
                        .join("\n");
                    let sentences = segments
                        .iter()
                        .map(|item| {
                            serde_json::json!({
                                "beginTime": item.begin_ms,
                                "endTime": item.end_ms,
                                "text": item.text,
                            })
                        })
                        .collect::<Vec<_>>();
                    serde_json::from_value(serde_json::json!({
                        "durationMs": duration_ms,
                        "transcripts": [{ "channelId": null, "text": text, "sentences": sentences }]
                    }))
                    .map_err(|error| error.to_string())
                })
                .await
                .map_err(|error| format!("本地文件识别任务失败：{error}"))??;
                if cancel
                    .as_ref()
                    .is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed))
                {
                    return Err("录音识别已取消".into());
                }
                Ok(result)
            }
        }
    }
    pub async fn upload(&self, model: &str, path: &str) -> Result<String, String> {
        match self {
            Self::AlibabaCloud { api_key, .. } => {
                alibabacloud::upload_for_model(api_key, model, path).await
            }
            Self::Plugin { .. } => Err("插件文件识别不使用宿主上传流程".into()),
            Self::Local { .. } => Err("本地文件识别不使用上传流程".into()),
        }
    }
    pub async fn submit(
        &self,
        model: &str,
        url: &str,
        params: &TranscriptionParams,
    ) -> Result<String, String> {
        match self {
            Self::AlibabaCloud {
                api_key,
                vocabulary_ids,
                ..
            } => {
                alibabacloud::submit_transcription_task(
                    api_key,
                    url,
                    params,
                    vocabulary_ids
                        .get(model)
                        .map(String::as_str)
                        .unwrap_or_default(),
                )
                .await
            }
            Self::Plugin { .. } => Err("插件文件识别不使用宿主异步任务流程".into()),
            Self::Local { .. } => Err("本地文件识别不使用异步任务流程".into()),
        }
    }
    pub async fn query(&self, id: &str) -> Result<TranscriptionTaskStatus, String> {
        match self {
            Self::AlibabaCloud { api_key, .. } => {
                alibabacloud::query_transcription_task(api_key, id).await
            }
            Self::Plugin { .. } => Err("插件文件识别不使用宿主轮询流程".into()),
            Self::Local { .. } => Err("本地文件识别不使用轮询流程".into()),
        }
    }
    pub async fn fetch(&self, url: &str) -> Result<TranscriptionResult, String> {
        match self {
            Self::AlibabaCloud { .. } => alibabacloud::fetch_transcription_result(url).await,
            Self::Plugin { .. } => Err("插件文件识别不使用宿主结果下载流程".into()),
            Self::Local { .. } => Err("本地文件识别不使用结果下载流程".into()),
        }
    }
}

/// 插件 OCR 识别的调用上限。图像最大 960px PNG（Base64 后远小于响应上限），
/// 在线 OCR API 往返通常在数秒内；60 秒足够覆盖慢速网络而不至于无限等待。
const PLUGIN_OCR_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub enum OcrProvider {
    /// Windows 系统 OCR（WinRT），转发 `crate::ocr::windows`。
    System,
    PpOcr {
        spec: LocalModelSpec,
    },
    Plugin {
        spec: PluginRuntimeSpec,
        profile: ProviderProfile,
    },
    Unavailable {
        selection: String,
        reason: String,
    },
}
impl OcrProvider {
    /// 通用 OCR 接口：输入 PNG 字节与用途标识（如 "activeAppContext"），
    /// 输出按 0~1 归一化坐标的文本块列表；排序、去重等收尾由消费方决定。
    pub async fn recognize(
        &self,
        image_png: &[u8],
        purpose: &str,
    ) -> Result<Vec<OcrTextBlock>, String> {
        match self {
            Self::System => {
                let png = image_png.to_vec();
                tokio::task::spawn_blocking(move || system_ocr_recognize(&png))
                    .await
                    .map_err(|error| format!("系统 OCR 工作线程失败：{error}"))?
            }
            Self::Plugin { spec, profile } => {
                let value = plugin_runtime::invoke(
                    spec,
                    profile,
                    "recognizeImage",
                    serde_json::json!({
                        "imageBase64":
                            base64::engine::general_purpose::STANDARD.encode(image_png),
                        "purpose": purpose,
                    }),
                    PLUGIN_OCR_TIMEOUT,
                    |_| {},
                )
                .await?;
                parse_plugin_ocr_blocks(&value)
            }
            Self::PpOcr { .. } => {
                Err("PP-OCR 由场景感知本地推理管线执行，不能走通用插件调用".into())
            }
            Self::Unavailable { reason, .. } => Err(reason.clone()),
        }
    }

    pub fn description(&self) -> String {
        match self {
            Self::System => "Windows 系统 OCR".into(),
            Self::PpOcr { spec } => format!("PP-OCR 模型 {}", spec.plugin_id),
            Self::Plugin { profile, .. } => profile.display_name.clone(),
            Self::Unavailable { selection, .. } => format!("不可用 OCR 模型 {selection}"),
        }
    }
}

fn system_ocr_recognize(png: &[u8]) -> Result<Vec<OcrTextBlock>, String> {
    #[cfg(windows)]
    {
        let image = image::load_from_memory(png)
            .map_err(|error| format!("解码 OCR 图像失败：{error}"))?;
        crate::ocr::windows::recognize(&image)
    }
    #[cfg(not(windows))]
    {
        let _ = png;
        Err("系统 OCR 仅在 Windows 上可用".into())
    }
}

/// v4 插件 `recognizeImage` 返回约定：`{ blocks: [{ text, region: { x, y, width, height }, confidence? }] }`，
/// 坐标为相对图像宽高的 0~1 归一化值；越界值会被收敛到合法区间。
fn parse_plugin_ocr_blocks(value: &Value) -> Result<Vec<OcrTextBlock>, String> {
    #[derive(Deserialize)]
    struct PluginOcrRegion {
        x: f32,
        y: f32,
        width: f32,
        height: f32,
    }
    #[derive(Deserialize)]
    struct PluginOcrBlock {
        text: String,
        region: PluginOcrRegion,
        #[serde(default)]
        confidence: Option<f32>,
    }
    let blocks = value
        .get("blocks")
        .cloned()
        .ok_or("插件 OCR 结果缺少 blocks 数组")?;
    let blocks: Vec<PluginOcrBlock> = serde_json::from_value(blocks).map_err(|error| {
        format!("插件 OCR 结果格式错误（期望 blocks[].text 与 region.x/y/width/height）：{error}")
    })?;
    Ok(blocks
        .into_iter()
        .filter_map(|block| {
            let text = crate::ocr::normalize_text(&block.text);
            if text.is_empty() {
                return None;
            }
            let bounds = NormalizedRegion {
                left: block.region.x,
                top: block.region.y,
                right: block.region.x + block.region.width,
                bottom: block.region.y + block.region.height,
            }
            .clamped();
            Some(OcrTextBlock {
                text,
                confidence: block
                    .confidence
                    .filter(|value| value.is_finite())
                    .unwrap_or(1.0),
                bounds,
            })
        })
        .collect())
}

#[derive(Clone)]
pub enum TranslationProvider {
    AlibabaCloud {
        api_key: String,
    },
    Plugin {
        spec: PluginRuntimeSpec,
        profile: ProviderProfile,
    },
}
#[cfg(test)]
pub fn translation_for(profile: &ProviderProfile) -> Result<TranslationProvider, CapabilityError> {
    translation_for_with_plugin(profile, None)
}
pub fn translation_for_with_plugin(
    profile: &ProviderProfile,
    plugin: Option<PluginRuntimeSpec>,
) -> Result<TranslationProvider, CapabilityError> {
    if let Some(spec) = plugin {
        return Ok(TranslationProvider::Plugin {
            spec,
            profile: profile.clone(),
        });
    }
    match profile.kind.as_str() {
        "alibabacloud-funasr" => Ok(TranslationProvider::AlibabaCloud {
            api_key: api_key(profile).map_err(|message| CapabilityError {
                provider_id: profile.id.clone(),
                capability: "translation",
                message,
            })?,
        }),
        _ => Err(unsupported(profile, "translation")),
    }
}
impl TranslationProvider {
    pub async fn translate_streaming<F>(
        &self,
        model: &str,
        text: &str,
        source: &str,
        target: &str,
        mut on_delta: F,
    ) -> Result<String, String>
    where
        F: FnMut(&str) + Send,
    {
        match self {
            Self::AlibabaCloud { api_key } => {
                alibabacloud::translate_streaming(api_key, model, text, source, target, on_delta)
                    .await
            }
            Self::Plugin { spec, profile } => {
                let value = plugin_runtime::invoke(
                    spec,
                    profile,
                    "translate",
                    serde_json::json!({
                        "model": model, "text": text, "source": source, "target": target
                    }),
                    Duration::from_secs(2 * 60),
                    |event| {
                        if let Some(text) = event.get("text").and_then(Value::as_str) {
                            on_delta(text);
                        }
                    },
                )
                .await?;
                value
                    .get("text")
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or_else(|| value.as_str().map(ToString::to_string))
                    .ok_or_else(|| "插件翻译结果缺少 text".to_string())
            }
        }
    }
}

#[derive(Clone)]
pub enum CustomizationProvider {
    AlibabaCloud {
        api_key: String,
    },
    Plugin {
        spec: PluginRuntimeSpec,
        profile: ProviderProfile,
    },
}
#[cfg(test)]
pub fn customization_for(
    profile: &ProviderProfile,
) -> Result<CustomizationProvider, CapabilityError> {
    customization_for_with_plugin(profile, None)
}
pub fn customization_for_with_plugin(
    profile: &ProviderProfile,
    plugin: Option<PluginRuntimeSpec>,
) -> Result<CustomizationProvider, CapabilityError> {
    if let Some(spec) = plugin {
        return Ok(CustomizationProvider::Plugin {
            spec,
            profile: profile.clone(),
        });
    }
    match profile.kind.as_str() {
        "alibabacloud-funasr" => Ok(CustomizationProvider::AlibabaCloud {
            api_key: profile
                .config
                .get("apiKey")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .trim()
                .to_string(),
        }),
        _ => Err(unsupported(profile, "customization")),
    }
}
impl CustomizationProvider {
    pub fn ensure_ready(&self) -> Result<(), String> {
        match self {
            Self::AlibabaCloud { api_key } if api_key.is_empty() => {
                Err("请先在设置中填写阿里云百炼 API Key".into())
            }
            Self::Plugin { .. } => Ok(()),
            _ => Ok(()),
        }
    }
    pub fn targets(&self) -> &'static [(&'static str, &'static str)] {
        match self {
            Self::AlibabaCloud { .. } => alibabacloud::VOCABULARY_TARGETS,
            Self::Plugin { .. } => &[],
        }
    }
    pub async fn create(
        &self,
        model: &str,
        prefix: &str,
        words: &[HotwordEntry],
    ) -> Result<String, String> {
        match self {
            Self::AlibabaCloud { api_key } => {
                alibabacloud::create_vocabulary(api_key, model, prefix, words).await
            }
            Self::Plugin { .. } => Err("插件热词使用统一 setHotwords 操作".into()),
        }
    }
    pub async fn update(&self, id: &str, words: &[HotwordEntry]) -> Result<(), String> {
        match self {
            Self::AlibabaCloud { api_key } => {
                alibabacloud::update_vocabulary(api_key, id, words).await
            }
            Self::Plugin { .. } => Err("插件热词使用统一 setHotwords 操作".into()),
        }
    }
    pub async fn delete(&self, id: &str) -> Result<(), String> {
        match self {
            Self::AlibabaCloud { api_key } => alibabacloud::delete_vocabulary(api_key, id).await,
            Self::Plugin { .. } => Err("插件热词使用统一 clearHotwords 操作".into()),
        }
    }
    pub async fn list(&self, prefix: &str) -> Result<Vec<String>, String> {
        match self {
            Self::AlibabaCloud { api_key } => alibabacloud::list_vocabulary(api_key, prefix).await,
            Self::Plugin { .. } => Err("插件热词使用统一 getHotwords 操作".into()),
        }
    }
    pub async fn query(&self, id: &str) -> Result<Vec<HotwordEntry>, String> {
        match self {
            Self::AlibabaCloud { api_key } => alibabacloud::query_vocabulary(api_key, id).await,
            Self::Plugin { .. } => Err("插件热词使用统一 getHotwords 操作".into()),
        }
    }

    pub fn is_plugin(&self) -> bool {
        matches!(self, Self::Plugin { .. })
    }

    pub async fn set_hotwords(&self, words: &[HotwordEntry]) -> Result<(), String> {
        let Self::Plugin { spec, profile } = self else {
            return Err("内置供应商不使用插件热词协议".into());
        };
        plugin_runtime::invoke(
            spec,
            profile,
            "setHotwords",
            serde_json::json!({ "hotwords": words }),
            plugin_runtime::DEFAULT_INVOKE_TIMEOUT,
            |_| {},
        )
        .await?;
        Ok(())
    }

    pub async fn get_hotwords(&self) -> Result<Vec<HotwordEntry>, String> {
        let Self::Plugin { spec, profile } = self else {
            return Err("内置供应商不使用插件热词协议".into());
        };
        let value = plugin_runtime::invoke(
            spec,
            profile,
            "getHotwords",
            serde_json::json!({}),
            plugin_runtime::DEFAULT_INVOKE_TIMEOUT,
            |_| {},
        )
        .await?;
        serde_json::from_value(value.get("hotwords").cloned().unwrap_or(value))
            .map_err(|error| format!("插件热词结果格式错误：{error}"))
    }

    pub async fn clear_hotwords(&self) -> Result<(), String> {
        let Self::Plugin { spec, profile } = self else {
            return Err("内置供应商不使用插件热词协议".into());
        };
        plugin_runtime::invoke(
            spec,
            profile,
            "clearHotwords",
            serde_json::json!({}),
            plugin_runtime::DEFAULT_INVOKE_TIMEOUT,
            |_| {},
        )
        .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn fake() -> ProviderProfile {
        ProviderProfile {
            id: "fake".into(),
            kind: "fake-kind".into(),
            display_name: "Fake".into(),
            auth_kind: "none".into(),
            capabilities: vec!["asr".into()],
            enabled: true,
            config: json!({}),
            config_fields: vec![],
            actions: vec![],
        }
    }
    #[test]
    fn missing_capability_is_structured() {
        let e = match file_recognition_for(&fake()) {
            Err(e) => e,
            Ok(_) => panic!(),
        };
        assert_eq!(
            (e.provider_id.as_str(), e.capability),
            ("fake", "fileRecognition")
        );
    }
    #[test]
    fn registered_provider_exposes_real_capabilities() {
        let mut p = super::super::funasr_profile();
        p.config["apiKey"] = json!("test");
        assert!(file_recognition_for(&p).is_ok());
        assert!(translation_for(&p).is_ok());
        assert!(customization_for(&p).is_ok());
    }

    #[test]
    fn plugin_ocr_blocks_are_normalized_and_clamped() {
        let value = json!({
            "blocks": [
                { "text": "  hello   world ", "region": { "x": 0.1, "y": 0.2, "width": 0.3, "height": 0.1 } },
                { "text": "overflow", "region": { "x": 0.9, "y": 0.9, "width": 0.5, "height": 0.5 }, "confidence": 0.75 },
                { "text": "   ", "region": { "x": 0.0, "y": 0.0, "width": 0.1, "height": 0.1 } }
            ]
        });
        let blocks = parse_plugin_ocr_blocks(&value).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text, "hello world");
        assert_eq!(blocks[0].confidence, 1.0);
        assert!((blocks[0].bounds.right - 0.4).abs() < 1e-6);
        assert_eq!(blocks[1].confidence, 0.75);
        assert_eq!(blocks[1].bounds.right, 1.0);
        assert_eq!(blocks[1].bounds.bottom, 1.0);

        assert!(parse_plugin_ocr_blocks(&json!({})).unwrap_err().contains("缺少 blocks"));
        assert!(parse_plugin_ocr_blocks(&json!({ "blocks": [{ "text": "x" }] }))
            .unwrap_err()
            .contains("格式错误"));
    }

    #[test]
    fn plugin_recognize_image_round_trips_payload_and_blocks() {
        let root = std::env::temp_dir().join(format!("sayit-ocr-plugin-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("connector")).unwrap();
        std::fs::write(
            root.join("connector/index.js"),
            concat!(
                "export default host => ({ invoke(request) {\n",
                "  if (request.operation !== 'recognizeImage') throw new Error('未知操作');\n",
                "  const { imageBase64, purpose } = request.payload;\n",
                "  const bytes = host.base64.decode(imageBase64);\n",
                "  return { blocks: [{ text: purpose + ':' + bytes.length, region: { x: 0.25, y: 0.5, width: 0.25, height: 0.1 } }] };\n",
                "} });",
            ),
        )
        .unwrap();
        let spec = PluginRuntimeSpec {
            plugin_id: "ocr-plugin".into(),
            root: root.clone(),
            entrypoint: root.join("connector/index.js"),
            permissions: vec![],
            allowed_hosts: vec![],
            browser_session: None,
            data_dir: root.join("data"),
            trust: "unsigned".into(),
        };
        let mut profile = fake();
        profile.capabilities = vec!["ocr".into()];
        let provider = OcrProvider::Plugin { spec, profile };
        let blocks = tauri::async_runtime::block_on(
            provider.recognize(&[1_u8, 2, 3, 4], "activeAppContext"),
        )
        .unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].text, "activeAppContext:4");
        assert!((blocks[0].bounds.left - 0.25).abs() < 1e-6);
        assert!((blocks[0].bounds.bottom - 0.6).abs() < 1e-6);
        std::fs::remove_dir_all(root).unwrap();
    }
}
