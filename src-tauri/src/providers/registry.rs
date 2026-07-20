use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use super::alibabacloud::RealtimeAsrFamily;

/// 模型注册表数据，编译期从仓库根的 shared/asr-models.json 内嵌。
static REGISTRY_JSON: &str = include_str!("../../../shared/asr-models.json");

/// 解析后的模型注册表，启动时解析一次，后续查询全部走静态引用。
static REGISTRY: Lazy<Vec<ModelInfo>> = Lazy::new(|| {
    serde_json::from_str(REGISTRY_JSON).expect("模型注册表 shared/asr-models.json 解析失败")
});

/// 模型元数据。字段与 `shared/asr-models.json` 及前端 `modelRegistry.ts` 的 `ModelInfo`
/// 一一对应，是前后端共享的数据契约：部分字段（如 `label`、`scenes`）仅前端消费，Rust 侧
/// 只需完整解析以保证注册表可加载，因此整体允许 dead_code。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)]
pub struct ModelInfo {
    pub id: String,
    pub label: String,
    /// 对应 `ProviderProfile.id`（如 `FUNASR_PROVIDER_ID`），不是 profile 的 `kind`。
    pub provider_id: String,
    pub category: String,
    pub protocol: String,
    pub supports_vocabulary: bool,
    /// 是否支持「上下文增强」：识别请求可以携带一段自然语言/词表文本，模型据此修正专有名词。
    ///
    /// 与 `supports_vocabulary` 相互独立：前者是带权重的词表接口，后者是纯文本上下文接口，
    /// 同一个模型可以两者都支持、都不支持或只支持其一。宿主按声明分别下发全局热词与全局上下文。
    ///
    /// 必须 `skip_serializing_if`，理由同 `emits_partial_results`：签名载荷由本结构体重新
    /// 序列化得到，缺省时若序列化成 `null` 会让既有插件签名集体失效。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_context: Option<bool>,
    pub supports_alignment_timestamps: bool,
    /// 识别过程中是否输出可变的中间结果（"边说边出字"）。
    ///
    /// `category` 只区分"实时会话"与"文件批处理"，装不下 VAD 分段的整句模型：这类模型
    /// 走实时会话，但说完一句才整句出字，没有中间态。省略时按 `category` 推导，保证既有
    /// 内置模型与旧插件清单行为不变。
    /// 必须 `skip_serializing_if`：签名载荷是对本结构体**重新序列化**得到的
    /// （见 `plugin_package::signing_payload`），若缺省时序列化成 `null`，所有在本字段
    /// 之前签名的插件载荷都会改变，签名集体验证失败。新增可选字段一律照此处理。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emits_partial_results: Option<bool>,
    pub scenes: Vec<String>,
    pub is_default_realtime: bool,
    pub is_default_file: bool,
}

impl ModelInfo {
    /// 未显式声明时的推导顺序：宿主自有引擎按实现判定，其余按 `category` 兜底。
    ///
    /// `local-sherpa-offline` 由宿主自己驱动，走 VAD 分段整句识别，实现上不存在中间结果
    /// （见 `commands/asr/local_session.rs` 的 Offline 分支）。这是宿主的事实而非插件的
    /// 声明，因此优先于 `category`——否则字段上线前已安装的模型包必须重新打包安装才能
    /// 拿到正确标注。
    pub fn emits_partial_results(&self) -> bool {
        if let Some(explicit) = self.emits_partial_results {
            return explicit;
        }
        if self.protocol == "local-sherpa-offline" {
            return false;
        }
        self.category == "realtime"
    }
}

pub fn models() -> &'static [ModelInfo] {
    REGISTRY.as_slice()
}

/// 从注册表查询模型信息；表外模型返回 None。
pub fn model_info(id: &str) -> Option<&'static ModelInfo> {
    let normalized = id.trim();
    REGISTRY.iter().find(|info| info.id == normalized)
}

/// 判断模型的实时识别协议族。表内模型直接查表，表外模型按前缀兜底。
pub fn realtime_asr_family(model: &str) -> RealtimeAsrFamily {
    if let Some(info) = model_info(model) {
        match info.protocol.as_str() {
            "qwen-realtime" => RealtimeAsrFamily::QwenRealtime,
            "dashscope-duplex" => RealtimeAsrFamily::DashscopeDuplex,
            _ => RealtimeAsrFamily::DashscopeDuplex, // 文件模型等异常情况兜底
        }
    } else {
        // 表外模型前缀兜底规则（与原 protocol.rs 逻辑一致）
        if model.trim().starts_with("qwen3-asr-flash-realtime") {
            RealtimeAsrFamily::QwenRealtime
        } else {
            RealtimeAsrFamily::DashscopeDuplex
        }
    }
}

/// 判断模型是否支持热词（vocabulary_id）。表内模型查表，表外模型按前缀兜底。
pub fn supports_vocabulary(model: &str) -> bool {
    if let Some(info) = model_info(model) {
        info.supports_vocabulary
    } else {
        // 表外模型前缀兜底规则（与原 protocol.rs 逻辑一致）
        let normalized = model.trim();
        normalized.starts_with("fun-asr") || normalized.starts_with("paraformer")
    }
}

/// 判断模型是否支持上下文增强。未声明按不支持处理：上下文只在明确声明的模型上生效，
/// 避免向不认识该字段的模型塞入无效内容。
pub fn supports_context(model: &str) -> bool {
    model_info(model)
        .and_then(|info| info.supports_context)
        .unwrap_or(false)
}

/// 判断模型是否支持对齐时间戳（文稿对齐场景需要）。
/// 目前该判断由前端 `modelRegistry.ts` 消费，后端保留同名查询以保持注册表 API 完整并被单测锁定。
#[allow(dead_code)]
pub fn supports_alignment_timestamps(model: &str) -> bool {
    model_info(model)
        .map(|info| info.supports_alignment_timestamps)
        .unwrap_or(false)
}

/// 文件转写模型的通路类型。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileTranscriptionRoute {
    /// 异步 OSS 上传 + 轮询（fun-asr、paraformer、qwen3-asr-flash-filetrans）
    AsyncOss,
    /// 同步短音频 fun-asr-flash SSE 流式
    SyncFunAsrFlash,
    /// 同步短音频 qwen3-asr-flash 非流式
    SyncQwen,
}

/// 判断文件转写模型走哪条通路。表内模型查表，表外模型按前缀兜底。
pub fn file_transcription_route(model: &str) -> FileTranscriptionRoute {
    if let Some(info) = model_info(model) {
        match info.protocol.as_str() {
            "file-async-oss" => FileTranscriptionRoute::AsyncOss,
            "file-sync-funasr-flash" => FileTranscriptionRoute::SyncFunAsrFlash,
            "file-sync-qwen" => FileTranscriptionRoute::SyncQwen,
            _ => FileTranscriptionRoute::AsyncOss, // 实时模型等异常情况兜底
        }
    } else {
        // 表外模型前缀兜底规则（与原 transcription.rs 逻辑一致）
        let normalized = model.trim();
        if normalized.starts_with("qwen3-asr-flash-filetrans") {
            FileTranscriptionRoute::AsyncOss
        } else if normalized == "qwen3-asr-flash" || normalized == "qwen3-asr-flash-2026-02-10" {
            FileTranscriptionRoute::SyncQwen
        } else if normalized == "fun-asr-flash-2026-06-15" {
            FileTranscriptionRoute::SyncFunAsrFlash
        } else if normalized.starts_with("paraformer") {
            FileTranscriptionRoute::AsyncOss
        } else {
            // 默认按 fun-asr 等模型走异步 OSS
            FileTranscriptionRoute::AsyncOss
        }
    }
}

/// 文件转写是否使用异步任务（决定是走轮询还是同步等待）。
pub fn uses_async_transcription_task(model: &str) -> bool {
    file_transcription_route(model) == FileTranscriptionRoute::AsyncOss
}

/// 获取默认的实时识别模型 ID。
pub fn default_realtime_model() -> &'static str {
    REGISTRY
        .iter()
        .find(|info| info.is_default_realtime)
        .map(|info| info.id.as_str())
        .unwrap_or("fun-asr-realtime")
}

/// 获取默认的文件识别模型 ID。
pub fn default_file_model() -> &'static str {
    REGISTRY
        .iter()
        .find(|info| info.is_default_file)
        .map(|info| info.id.as_str())
        .unwrap_or("fun-asr")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_parses() {
        // 确保注册表能正常解析
        let _ = &*REGISTRY;
        assert!(!REGISTRY.is_empty(), "注册表不应为空");
    }

    #[test]
    fn test_model_count() {
        // 验证现有 9 个模型都在表内
        assert_eq!(REGISTRY.len(), 9, "当前应有 9 个模型");
    }

    #[test]
    fn test_realtime_family_in_table() {
        // 表内模型：fun-asr-realtime 走 DashscopeDuplex
        assert_eq!(
            realtime_asr_family("fun-asr-realtime"),
            RealtimeAsrFamily::DashscopeDuplex
        );
        // 表内模型：qwen3-asr-flash-realtime 走 QwenRealtime
        assert_eq!(
            realtime_asr_family("qwen3-asr-flash-realtime"),
            RealtimeAsrFamily::QwenRealtime
        );
    }

    #[test]
    fn test_realtime_family_fallback() {
        // 表外模型：qwen3-asr-flash-realtime-未知版本 按前缀兜底
        assert_eq!(
            realtime_asr_family("qwen3-asr-flash-realtime-9999"),
            RealtimeAsrFamily::QwenRealtime
        );
        // 表外模型：其他模型兜底为 DashscopeDuplex
        assert_eq!(
            realtime_asr_family("unknown-model"),
            RealtimeAsrFamily::DashscopeDuplex
        );
    }

    #[test]
    fn test_supports_vocabulary_in_table() {
        // 表内模型：fun-asr-realtime 支持热词
        assert!(supports_vocabulary("fun-asr-realtime"));
        // 表内模型：qwen3-asr-flash-realtime 不支持热词
        assert!(!supports_vocabulary("qwen3-asr-flash-realtime"));
    }

    #[test]
    fn test_supports_vocabulary_fallback() {
        // 表外模型：fun-asr 前缀支持
        assert!(supports_vocabulary("fun-asr-unknown"));
        // 表外模型：paraformer 前缀支持
        assert!(supports_vocabulary("paraformer-unknown"));
        // 表外模型：其他不支持
        assert!(!supports_vocabulary("unknown-model"));
    }

    #[test]
    fn test_supports_context() {
        // 表内声明支持上下文增强的模型
        assert!(supports_context("fun-asr-flash-2026-06-15"));
        // 表内未声明的模型按不支持处理
        assert!(!supports_context("fun-asr-realtime"));
        // 表外模型不做前缀兜底，一律不支持
        assert!(!supports_context("fun-asr-unknown"));
    }

    #[test]
    fn test_file_transcription_route() {
        // fun-asr 走异步 OSS
        assert_eq!(
            file_transcription_route("fun-asr"),
            FileTranscriptionRoute::AsyncOss
        );
        // fun-asr-flash-2026-06-15 走同步 FunAsrFlash
        assert_eq!(
            file_transcription_route("fun-asr-flash-2026-06-15"),
            FileTranscriptionRoute::SyncFunAsrFlash
        );
        // qwen3-asr-flash 走同步 Qwen
        assert_eq!(
            file_transcription_route("qwen3-asr-flash"),
            FileTranscriptionRoute::SyncQwen
        );
        // qwen3-asr-flash-filetrans 走异步 OSS
        assert_eq!(
            file_transcription_route("qwen3-asr-flash-filetrans"),
            FileTranscriptionRoute::AsyncOss
        );
    }

    #[test]
    fn test_file_route_fallback() {
        // 表外模型：qwen3-asr-flash-filetrans 前缀
        assert_eq!(
            file_transcription_route("qwen3-asr-flash-filetrans-9999"),
            FileTranscriptionRoute::AsyncOss
        );
        // 表外模型：paraformer 前缀
        assert_eq!(
            file_transcription_route("paraformer-9999"),
            FileTranscriptionRoute::AsyncOss
        );
        // 表外模型：未知模型兜底
        assert_eq!(
            file_transcription_route("unknown-model"),
            FileTranscriptionRoute::AsyncOss
        );
    }

    #[test]
    fn test_uses_async_transcription_task() {
        // 异步 OSS 模型返回 true
        assert!(uses_async_transcription_task("fun-asr"));
        assert!(uses_async_transcription_task("qwen3-asr-flash-filetrans"));
        // 同步模型返回 false
        assert!(!uses_async_transcription_task("fun-asr-flash-2026-06-15"));
        assert!(!uses_async_transcription_task("qwen3-asr-flash"));
    }

    #[test]
    fn test_supports_alignment_timestamps() {
        // fun-asr 支持对齐时间戳
        assert!(supports_alignment_timestamps("fun-asr"));
        // qwen3-asr-flash-filetrans 支持对齐时间戳
        assert!(supports_alignment_timestamps("qwen3-asr-flash-filetrans"));
        // fun-asr-flash-2026-06-15 支持对齐时间戳
        assert!(supports_alignment_timestamps("fun-asr-flash-2026-06-15"));
        // qwen3-asr-flash 不支持对齐时间戳（无 words 字段）
        assert!(!supports_alignment_timestamps("qwen3-asr-flash"));
        // 实时模型不支持对齐时间戳
        assert!(!supports_alignment_timestamps("fun-asr-realtime"));
    }

    #[test]
    fn test_default_models() {
        // 默认实时模型
        assert_eq!(default_realtime_model(), "fun-asr-realtime-2026-02-28");
        // 默认文件模型
        assert_eq!(default_file_model(), "fun-asr-flash-2026-06-15");
    }

    #[test]
    fn test_model_info() {
        // 表内模型能查到
        let info = model_info("fun-asr-realtime");
        assert!(info.is_some());
        assert_eq!(info.unwrap().label, "Fun-ASR-Realtime 稳定版");
        // 表外模型返回 None
        assert!(model_info("unknown-model").is_none());
    }
}
