use super::*;
use crate::state::RuntimeState;

pub(super) const PROVIDER: &str = "acceptance-provider";
pub(super) const FILE_A: &str = "acceptance-file-a";
pub(super) const FILE_B: &str = "acceptance-file-b";
pub(super) const LIVE: &str = "acceptance-live";

pub(super) struct Fixture {
    pub storage: PathBuf,
}

impl Fixture {
    pub async fn install(app: &AppHandle) -> Result<Self, String> {
        let base = output_path()?
            .parent()
            .ok_or("验收目录无父目录")?
            .join("fixture");
        let plugins = base.join("plugins");
        let root = plugins.join(PROVIDER);
        std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let definitions = [
            (FILE_A, "request-response"),
            (FILE_B, "request-response"),
            (LIVE, "realtime"),
        ];
        let capabilities: Vec<_> = definitions.iter().map(|(model, mode)| json!({
            "moduleId":format!("{PROVIDER}.speech-recognition.{model}"), "kind":"speech-recognition",
            "providerIds":[PROVIDER], "modelId":model, "operations":["speech-recognition"],
            "executionModes":[mode], "features":[], "tags":["acceptance"]
        })).collect();
        let models: Vec<_> = definitions
            .iter()
            .map(|(model, _)| {
                json!({
                    "id":model,"label":model,"providerId":PROVIDER,
                    "capabilityId":format!("{PROVIDER}.speech-recognition.{model}")
                })
            })
            .collect();
        let manifest = json!({
            "apiVersion":5,"id":PROVIDER,"name":"本地验收供应商","version":"1.0.0",
            "provider":{"id":PROVIDER,"displayName":"本地验收供应商","authKind":"none","capabilities":["asr"],"config":{}},
            "source":{"namespace":PROVIDER},"capabilities":capabilities,"models":models,
            "runtime":{"kind":"javascript","entrypoint":"provider.js","hostApiVersion":1,"permissions":[],"network":{"allowedHosts":[]}}
        });
        std::fs::write(
            root.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::write(root.join("provider.js"), include_str!("provider.js"))
            .map_err(|e| e.to_string())?;
        let registry = tauri::async_runtime::spawn_blocking(move || {
            crate::providers::plugin::load_registry_from(&plugins)
        })
        .await
        .map_err(|e| e.to_string())??;
        let snapshot = registry.snapshot_with_provider_settings(None);
        if !snapshot.errors.is_empty()
            || registry.model(FILE_A).is_none()
            || registry.model(LIVE).is_none()
        {
            return Err(format!(
                "验收插件未通过正式注册校验：{}",
                serde_json::to_string(&snapshot.errors).map_err(|e| e.to_string())?
            ));
        }
        let storage = registry
            .runtime_for_provider(PROVIDER)?
            .ok_or("验收插件运行时不存在")?
            .data_dir
            .join("storage.json");
        let state = app.state::<RuntimeState>();
        {
            let mut providers = state.providers.lock().map_err(|_| "供应商状态锁失败")?;
            registry.merge_provider_profiles(&mut providers);
        }
        *state.plugin_registry.lock().map_err(|_| "插件状态锁失败")? = registry;
        Ok(Self { storage })
    }

    pub fn configure(&self, app: &AppHandle, mode: &str) -> Result<String, String> {
        let nonce = uuid::Uuid::new_v4().to_string();
        let state = app.state::<RuntimeState>();
        let mut providers = state.providers.lock().map_err(|_| "供应商状态锁失败")?;
        providers
            .profiles
            .iter_mut()
            .find(|profile| profile.id == PROVIDER)
            .ok_or("验收供应商丢失")?
            .config = json!({"mode":mode,"nonce":nonce});
        Ok(nonce)
    }

    pub async fn wait_ready(&self, nonce: &str) -> Result<(), String> {
        tokio::time::timeout(Duration::from_secs(20), async {
            loop {
                if let Ok(bytes) = std::fs::read(&self.storage) {
                    let value: serde_json::Value =
                        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    if value["ready"] == nonce {
                        return Ok(());
                    }
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .map_err(|_| "等待验收插件实际读取输入超时".to_string())?
    }
}

pub(super) async fn audio(seconds: usize) -> Result<crate::audio_wav::RecordedWav, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut wav =
            crate::audio_wav::WavRecording::new(16_000, crate::audio_wav::Quantization::Round)
                .map_err(|e| e.to_string())?;
        let chunk: Vec<f32> = (0..1600)
            .map(|index| (index as f32 / 1600.0 - 0.5) * 0.2)
            .collect();
        for _ in 0..seconds * 10 {
            wav.append(&chunk).map_err(|e| e.to_string())?;
        }
        wav.finish().map_err(|e| e.to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

pub(super) async fn wait_jobs_empty(app: &AppHandle) -> Result<(), String> {
    tokio::time::timeout(Duration::from_secs(120), async {
        loop {
            let state = app.state::<RuntimeState>();
            if state
                .transcriptions
                .lock()
                .map_err(|_| "任务状态锁失败")?
                .is_empty()
                && state
                    .asr_streams
                    .lock()
                    .map_err(|_| "识别状态锁失败")?
                    .is_empty()
            {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .map_err(|_| "识别任务未完成清理".to_string())?
}
