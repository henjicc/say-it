//! transcribe.cpp（ggml）整句识别：GGUF 权重，有可用 GPU 时走 Vulkan/Metal，否则用 CPU。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use transcribe_cpp::{
    Backend, CancelToken, DeviceType, Error, Model, ModelOptions, RunOptions, Session,
    SessionOptions,
};

use super::{check_cancel, ensure_ready, int_param, param_path, string_param, SAMPLE_RATE};
use crate::cancellation::CancellationFlag;
use crate::providers::plugin::LocalModelSpec;
use crate::providers::RequestCustomization;

const CANCELLED: &str = "本地语音识别已取消";

pub struct TranscribeRecognizer {
    // 会话持有模型引用；一个会话同一时刻只能跑一次识别。
    session: Mutex<Session>,
    cancel: CancelToken,
    language: Option<String>,
    on_gpu: bool,
}

/// 后端模块只能在进程内登记一次，且必须早于任何模型加载。
fn init_backends() -> Result<(), String> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    INIT.get_or_init(|| {
        transcribe_cpp::init_logging();
        transcribe_cpp::init_backends_default()
            .map_err(|error| format!("加载本地推理后端失败：{error}"))
    })
    .clone()
}

pub(super) fn create_recognizer(spec: &LocalModelSpec) -> Result<TranscribeRecognizer, String> {
    ensure_ready(spec)?;
    init_backends()?;
    // 显卡驱动异常或显存不足时仍要能用，只是慢一些。
    open(spec, Backend::Auto).or_else(|gpu_error| {
        let recognizer = open(spec, Backend::Cpu)?;
        crate::application::diagnostics::event(
            "warn",
            "localAsr.gpuFallback",
            serde_json::json!({ "pluginId": spec.plugin_id, "message": gpu_error }),
        );
        Ok(recognizer)
    })
}

fn open(spec: &LocalModelSpec, backend: Backend) -> Result<TranscribeRecognizer, String> {
    let options = ModelOptions {
        backend,
        ..Default::default()
    };
    let model = Model::load_with(param_path(spec, "model")?, &options)
        .map_err(|error| format!("加载本地模型失败：{error}"))?;
    let device = model
        .device()
        .map_err(|error| format!("读取本地推理设备失败：{error}"))?;
    let on_gpu = matches!(device.device_type, DeviceType::Gpu | DeviceType::Igpu);
    let mut session = model
        .session_with(&SessionOptions {
            n_threads: int_param(&spec.params, "numThreads", 0),
            ..Default::default()
        })
        .map_err(|error| format!("创建本地识别会话失败：{error}"))?;
    let cancel = CancelToken::new();
    session.set_cancel_token(&cancel);
    let language = Some(string_param(&spec.params, "language", "auto")).filter(|v| v != "auto");
    let recognizer = TranscribeRecognizer {
        session: Mutex::new(session),
        cancel,
        language,
        on_gpu,
    };
    // GPU 后端在第一次推理时编译着色器，新装显卡驱动后首次可达十秒。放在加载阶段做，
    // 与用户说话的时间重叠，而不是压在松开快捷键之后。
    recognizer.recognize(
        &vec![0.0; SAMPLE_RATE as usize],
        &RequestCustomization::default(),
        None,
    )?;
    Ok(recognizer)
}

impl TranscribeRecognizer {
    /// 整句识别只要几百毫秒，可以在说话过程中反复识别当前这句话来给出预览。
    pub(super) fn fast_enough_for_preview(&self) -> bool {
        self.on_gpu
    }

    pub(super) fn recognize(
        &self,
        samples: &[f32],
        customization: &RequestCustomization,
        cancel: Option<&CancellationFlag>,
    ) -> Result<String, String> {
        let mut session = self.session.lock().map_err(|_| "本地识别器锁失败")?;
        check_cancel(cancel)?;
        self.cancel.reset();
        let options = RunOptions {
            language: self.language.clone(),
            vocabulary: customization
                .hotwords
                .iter()
                .map(|item| item.text.clone())
                .collect(),
            prompt: Some(customization.context.clone()).filter(|text| !text.is_empty()),
            ..Default::default()
        };
        let done = AtomicBool::new(false);
        let result = std::thread::scope(|scope| {
            // 原生库在解码步之间轮询自己的取消标记，这里把会话的取消转交过去。
            let watcher = cancel.map(|flag| {
                scope.spawn(|| {
                    while !done.load(Ordering::Acquire) {
                        if flag.load(Ordering::Acquire) {
                            self.cancel.cancel();
                            return;
                        }
                        std::thread::park_timeout(Duration::from_millis(20));
                    }
                })
            });
            let result = session.run(samples, &options);
            done.store(true, Ordering::Release);
            if let Some(watcher) = watcher {
                watcher.thread().unpark();
            }
            result
        });
        match result {
            Ok(transcript) => Ok(transcript.text.trim().to_string()),
            Err(Error::Aborted { .. }) => Err(CANCELLED.into()),
            // 生成预算用尽或输出开始重复时，已得到的部分仍是有效文本。
            Err(error @ (Error::OutputTruncated { .. } | Error::OutputRepetition { .. })) => {
                crate::application::diagnostics::event(
                    "warn",
                    "localAsr.incompleteOutput",
                    serde_json::json!({ "message": error.to_string() }),
                );
                Ok(error
                    .partial()
                    .map(|transcript| transcript.text.trim().to_string())
                    .unwrap_or_default())
            }
            Err(error) => Err(format!("本地语音识别失败：{error}")),
        }
    }
}
