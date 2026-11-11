//! 仅实时会话共享识别器：开了常驻的一直留着，没开的用完再留一小段时间；
//! 文件识别独占临时识别器，音频流与 VAD 归会话所有。
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use serde::Serialize;
use sherpa_onnx::{OfflinePunctuation, OnlineRecognizer};

use crate::providers::plugin::LocalModelSpec;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct ModelKey {
    plugin_id: String,
    version: String,
    model_dir: std::path::PathBuf,
    engine: String,
    files: Vec<(String, String, u64)>,
    params: String,
}

impl From<&LocalModelSpec> for ModelKey {
    fn from(spec: &LocalModelSpec) -> Self {
        let mut files = spec
            .files
            .iter()
            .map(|file| (file.path.clone(), file.sha256.clone(), file.size_bytes))
            .collect::<Vec<_>>();
        files.sort();
        Self {
            plugin_id: spec.plugin_id.clone(),
            version: spec.manifest_version.clone(),
            model_dir: spec.model_dir.clone(),
            engine: spec.engine.clone(),
            files,
            params: spec.params.to_string(),
        }
    }
}

pub(super) enum Recognizer {
    Segment(super::SegmentRecognizer),
    Online(Arc<Mutex<OnlineRecognizer>>),
    Punctuation(Arc<Mutex<OfflinePunctuation>>),
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResidencyStatus {
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

enum LoadState<T> {
    Loading,
    Ready(Arc<T>),
    Failed(String),
}

struct Entry<T> {
    state: Mutex<LoadState<T>>,
}

/// 没开常驻的模型从一次实时会话开始起继续留在内存里的时长：连续听写不必每次重新加载，
/// 这么久没有新的会话用到它就释放。
const LINGER: std::time::Duration = std::time::Duration::from_secs(5 * 60);

struct Lingering<T> {
    value: Arc<T>,
    /// 每次使用都换一个新值，过期任务据此判断期间有没有被再次用到。
    turn: u64,
}

struct ResidentCache<T> {
    entries: HashMap<ModelKey, Arc<Entry<T>>>,
    suspended: HashMap<String, usize>,
    disabled: bool,
    all_suspended: usize,
    lingering: HashMap<ModelKey, Lingering<T>>,
    turns: u64,
}

impl<T> Default for ResidentCache<T> {
    fn default() -> Self {
        Self {
            entries: HashMap::new(),
            suspended: HashMap::new(),
            disabled: false,
            all_suspended: 0,
            lingering: HashMap::new(),
            turns: 0,
        }
    }
}

impl<T> ResidentCache<T> {
    fn reconcile(&mut self, specs: Vec<LocalModelSpec>) -> Vec<(LocalModelSpec, Arc<Entry<T>>)> {
        let desired: HashMap<_, _> = specs
            .into_iter()
            .filter(|spec| !self.blocked(&spec.plugin_id))
            .map(|spec| (ModelKey::from(&spec), spec))
            .collect();
        self.entries.retain(|key, _| desired.contains_key(key));
        let mut jobs = Vec::new();
        for (key, spec) in desired {
            // 刚用过还留在内存里的同一个模型直接转为常驻，不再加载第二份。
            let lingering = self.lingering.remove(&key);
            self.entries.entry(key).or_insert_with(|| {
                let state = match lingering {
                    Some(item) => LoadState::Ready(item.value),
                    None => LoadState::Loading,
                };
                let loading = matches!(state, LoadState::Loading);
                let entry = Arc::new(Entry {
                    state: Mutex::new(state),
                });
                if loading {
                    jobs.push((spec, entry.clone()));
                }
                entry
            });
        }
        // 启用状态刚变过，停用的包不能继续占着内存。
        self.lingering.clear();
        jobs
    }

    fn blocked(&self, plugin_id: &str) -> bool {
        self.disabled || self.all_suspended > 0 || self.suspended.contains_key(plugin_id)
    }

    /// 取出仍留在内存里的同一个模型，并把它的释放时间往后推。
    fn reuse_lingering(&mut self, key: &ModelKey) -> Option<(Arc<T>, u64)> {
        let lingering = self.lingering.get_mut(key)?;
        self.turns += 1;
        lingering.turn = self.turns;
        Some((lingering.value.clone(), lingering.turn))
    }

    /// 包正在替换、数据根已迁移，或这个模型已开常驻（预载还没完成）时不留，返回 `None`：
    /// 前两种情况不能再占着文件，后一种等预载完成就会有两份。
    fn linger(&mut self, spec: &LocalModelSpec, value: Arc<T>) -> Option<u64> {
        let key = ModelKey::from(spec);
        if self.blocked(&spec.plugin_id) || self.entries.contains_key(&key) {
            return None;
        }
        self.turns += 1;
        let turn = self.turns;
        self.lingering.insert(key, Lingering { value, turn });
        Some(turn)
    }

    fn expire_lingering(&mut self, key: &ModelKey, turn: u64) {
        if self
            .lingering
            .get(key)
            .is_some_and(|item| item.turn == turn)
        {
            self.lingering.remove(key);
        }
    }

    fn get(&self, spec: &LocalModelSpec) -> Result<Option<Arc<T>>, String> {
        let Some(entry) = self.entries.get(&ModelKey::from(spec)) else {
            return Ok(None);
        };
        let state = entry.state.lock().map_err(|_| "本地模型加载状态锁失败")?;
        Ok(match &*state {
            LoadState::Ready(value) => Some(value.clone()),
            LoadState::Loading | LoadState::Failed(_) => None,
        })
    }
}

fn cache() -> &'static Mutex<ResidentCache<Recognizer>> {
    static CACHE: OnceLock<Mutex<ResidentCache<Recognizer>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(ResidentCache::default()))
}

pub fn supports_residency(engine: &str) -> bool {
    matches!(
        engine,
        "sherpa-onnx-offline"
            | "transcribe-cpp"
            | "sherpa-onnx-online"
            | "sherpa-onnx-punctuation"
            | crate::providers::local_llm::ENGINE
    )
}

fn load(spec: &LocalModelSpec) -> Result<Recognizer, String> {
    match spec.engine.as_str() {
        "sherpa-onnx-offline" | "transcribe-cpp" => {
            super::create_segment_recognizer(spec).map(Recognizer::Segment)
        }
        "sherpa-onnx-online" => super::create_online_recognizer(spec)
            .map(|value| Recognizer::Online(Arc::new(Mutex::new(value)))),
        "sherpa-onnx-punctuation" => super::punctuation::create_punctuator(spec)
            .map(|value| Recognizer::Punctuation(Arc::new(Mutex::new(value)))),
        engine => Err(format!("模型引擎 {engine} 不支持常驻内存")),
    }
}

pub(super) fn acquire(spec: &LocalModelSpec) -> Result<Arc<Recognizer>, String> {
    // 全局锁只查询和登记缓存，不覆盖模型加载或实时解码。
    let lock = || cache().lock().map_err(|_| "本地模型缓存锁失败");
    let key = ModelKey::from(spec);
    let (value, turn) = {
        let mut cache = lock()?;
        if let Some(value) = cache.get(spec)? {
            return Ok(value);
        }
        match cache.reuse_lingering(&key) {
            Some((value, turn)) => (value, Some(turn)),
            None => {
                drop(cache);
                let value = Arc::new(load(spec)?);
                let turn = lock()?.linger(spec, value.clone());
                (value, turn)
            }
        }
    };
    if let Some(turn) = turn {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(LINGER).await;
            match cache().lock() {
                Ok(mut cache) => cache.expire_lingering(&key, turn),
                Err(error) => eprintln!("[local-asr] 释放闲置模型失败：{error}"),
            }
        });
    }
    Ok(value)
}

pub fn status(spec: &LocalModelSpec) -> Result<ResidencyStatus, String> {
    let cache = cache().lock().map_err(|_| "本地模型缓存锁失败")?;
    let Some(entry) = cache.entries.get(&ModelKey::from(spec)) else {
        return Ok(ResidencyStatus {
            state: "unloaded".into(),
            error: None,
        });
    };
    let state = entry.state.lock().map_err(|_| "本地模型加载状态锁失败")?;
    Ok(match &*state {
        LoadState::Loading => ResidencyStatus {
            state: "loading".into(),
            error: None,
        },
        LoadState::Ready(_) => ResidencyStatus {
            state: "loaded".into(),
            error: None,
        },
        LoadState::Failed(error) => ResidencyStatus {
            state: "failed".into(),
            error: Some(error.clone()),
        },
    })
}

pub fn reconcile(specs: Vec<LocalModelSpec>, app: &tauri::AppHandle) -> Result<(), String> {
    let jobs = cache()
        .lock()
        .map_err(|_| "本地模型缓存锁失败")?
        .reconcile(specs);
    for (spec, entry) in jobs {
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let result = load(&spec);
            if let Err(error) = &result {
                crate::application::diagnostics::event(
                    "error",
                    "localAsr.preloadFailed",
                    serde_json::json!({ "pluginId": spec.plugin_id, "message": error }),
                );
            }
            match entry.state.lock() {
                Ok(mut state) => {
                    *state = match result {
                        Ok(value) => LoadState::Ready(Arc::new(value)),
                        Err(error) => LoadState::Failed(error),
                    }
                }
                Err(error) => eprintln!("[local-asr] 写入加载状态失败：{error}"),
            }
            use tauri::Emitter;
            if let Err(error) = app.emit("plugin-registry-changed", ()) {
                eprintln!("[local-asr] 通知常驻状态失败：{error}");
            }
        });
    }
    Ok(())
}

/// 包文件正在替换时禁止重新预载；完成后的注册表同步负责再次加载。
pub struct Suspension(Option<String>);

pub fn suspend(plugin_id: &str) -> Result<Suspension, String> {
    let mut cache = cache().lock().map_err(|_| "本地模型缓存锁失败")?;
    *cache.suspended.entry(plugin_id.into()).or_default() += 1;
    cache.entries.retain(|key, _| key.plugin_id != plugin_id);
    cache.lingering.retain(|key, _| key.plugin_id != plugin_id);
    Ok(Suspension(Some(plugin_id.into())))
}

impl Drop for Suspension {
    fn drop(&mut self) {
        match cache().lock() {
            Ok(mut cache) => {
                if let Some(id) = &self.0 {
                    if let Some(count) = cache.suspended.get_mut(id) {
                        *count -= 1;
                        if *count == 0 {
                            cache.suspended.remove(id);
                        }
                    }
                } else {
                    cache.all_suspended -= 1;
                }
            }
            Err(error) => eprintln!("[local-asr] 恢复模型缓存失败：{error}"),
        }
    }
}

/// 数据根已迁移，进程等待重启期间不能重新持有旧目录的模型。
pub fn disable_until_restart() -> Result<(), String> {
    let mut cache = cache().lock().map_err(|_| "本地模型缓存锁失败")?;
    cache.disabled = true;
    cache.entries.clear();
    cache.lingering.clear();
    Ok(())
}

pub fn suspend_all() -> Result<Suspension, String> {
    let mut cache = cache().lock().map_err(|_| "本地模型缓存锁失败")?;
    cache.all_suspended += 1;
    cache.entries.clear();
    cache.lingering.clear();
    Ok(Suspension(None))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::plugin::ModelPackFileManifest;

    fn spec() -> LocalModelSpec {
        LocalModelSpec {
            manifest_version: "1".into(),
            plugin_id: "test".into(),
            provider_id: "local".into(),
            engine: "sherpa-onnx-offline".into(),
            model_dir: "temporary-models".into(),
            files: vec![ModelPackFileManifest {
                path: "model".into(),
                sha256: "a".repeat(64),
                size_bytes: 1,
                download: None,
            }],
            params: serde_json::json!({"numThreads":4}),
        }
    }

    #[test]
    fn cache_hit_reuses_loaded_recognizer_without_loading() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        let jobs = cache.reconcile(vec![spec.clone()]);
        let value = Arc::new(42);
        *jobs[0].1.state.lock().unwrap() = LoadState::Ready(value.clone());
        let actual = cache.get(&spec).unwrap().unwrap();
        assert!(Arc::ptr_eq(&value, &actual));
        assert!(cache.reconcile(vec![spec]).is_empty());
    }

    #[test]
    fn every_model_identity_component_prevents_reuse() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        let jobs = cache.reconcile(vec![spec.clone()]);
        *jobs[0].1.state.lock().unwrap() = LoadState::Ready(Arc::new(42));
        let mut variants = vec![spec.clone(); 7];
        variants[0].plugin_id.push('2');
        variants[1].manifest_version.push('2');
        variants[2].model_dir.push("new");
        variants[3].files[0].sha256 = "b".repeat(64);
        variants[4].engine = "sherpa-onnx-online".into();
        variants[5].params["numThreads"] = 2.into();
        variants[6].files[0].size_bytes += 1;
        for variant in variants {
            assert!(cache.get(&variant).unwrap().is_none());
        }
    }

    #[test]
    fn eviction_keeps_active_session_alive_until_its_last_reference() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        let jobs = cache.reconcile(vec![spec.clone()]);
        *jobs[0].1.state.lock().unwrap() = LoadState::Ready(Arc::new(42));
        drop(jobs);
        let session = cache.get(&spec).unwrap().unwrap();
        let weak = Arc::downgrade(&session);
        cache.reconcile(vec![]);
        assert!(cache.get(&spec).unwrap().is_none());
        assert_eq!(*session, 42);
        drop(session);
        assert!(weak.upgrade().is_none());
    }

    #[test]
    fn late_preload_cannot_resurrect_evicted_or_updated_model() {
        let mut spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        let jobs = cache.reconcile(vec![spec.clone()]);
        spec.manifest_version.push('2');
        let updated = cache.reconcile(vec![spec.clone()]);
        *jobs[0].1.state.lock().unwrap() = LoadState::Ready(Arc::new(1));
        assert!(cache.get(&spec).unwrap().is_none());
        *updated[0].1.state.lock().unwrap() = LoadState::Ready(Arc::new(2));
        assert_eq!(*cache.get(&spec).unwrap().unwrap(), 2);
        cache.reconcile(vec![]);
        assert!(cache.entries.is_empty());
    }

    #[test]
    fn failed_or_loading_preload_is_not_served_from_cache() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        let jobs = cache.reconcile(vec![spec.clone()]);
        assert!(cache.get(&spec).unwrap().is_none());
        *jobs[0].1.state.lock().unwrap() = LoadState::Failed("校验失败".into());
        assert!(cache.get(&spec).unwrap().is_none());
    }

    #[test]
    fn lingering_model_is_reused_until_an_unused_period_expires() {
        let spec = spec();
        let key = ModelKey::from(&spec);
        let mut cache = ResidentCache::<usize>::default();
        let value = Arc::new(7);
        let first = cache.linger(&spec, value.clone()).unwrap();
        let (reused, second) = cache.reuse_lingering(&key).unwrap();
        assert!(Arc::ptr_eq(&value, &reused));
        // 第一次使用排下的过期任务到点时，模型期间又被用过，不能释放。
        cache.expire_lingering(&key, first);
        assert!(cache.lingering.contains_key(&key));
        cache.expire_lingering(&key, second);
        assert!(cache.reuse_lingering(&key).is_none());
    }

    #[test]
    fn turning_on_residency_adopts_the_lingering_model_instead_of_loading_twice() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        let value = Arc::new(7);
        cache.linger(&spec, value.clone()).unwrap();
        assert!(cache.reconcile(vec![spec.clone()]).is_empty());
        assert!(Arc::ptr_eq(&value, &cache.get(&spec).unwrap().unwrap()));
        // 已经是常驻模型：预载没完成时临时加载的那份用完即弃。
        assert!(cache.linger(&spec, Arc::new(8)).is_none());
        assert!(cache.lingering.is_empty());
    }

    #[test]
    fn replacing_a_pack_drops_its_lingering_model() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        cache.suspended.insert(spec.plugin_id.clone(), 1);
        assert!(cache.linger(&spec, Arc::new(7)).is_none());
        assert!(cache.lingering.is_empty());
    }

    #[test]
    fn replacement_and_data_root_change_block_preloading() {
        let spec = spec();
        let mut cache = ResidentCache::<usize>::default();
        cache.suspended.insert(spec.plugin_id.clone(), 1);
        assert!(cache.reconcile(vec![spec.clone()]).is_empty());
        cache.suspended.clear();
        assert_eq!(cache.reconcile(vec![spec.clone()]).len(), 1);
        cache.disabled = true;
        assert!(cache.reconcile(vec![spec]).is_empty());
        assert!(cache.entries.is_empty());
    }
    #[cfg(target_os = "windows")]
    fn sherpa(
        recognizer: &super::super::SegmentRecognizer,
    ) -> &Arc<Mutex<sherpa_onnx::OfflineRecognizer>> {
        let super::super::SegmentRecognizer::Sherpa(recognizer) = recognizer else {
            panic!("需要 sherpa 离线模型")
        };
        recognizer
    }

    #[test]
    #[cfg(target_os = "windows")]
    #[ignore = "需要 SAYIT_RESIDENCY_DESCRIPTOR、SAYIT_RESIDENCY_MODEL_DIR、SAYIT_RESIDENCY_WAV；独立进程测量"]
    fn real_model_residency_probe() {
        use std::time::Instant;
        let descriptor = std::env::var("SAYIT_RESIDENCY_DESCRIPTOR").unwrap();
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&descriptor).unwrap()).unwrap();
        let manifest: crate::providers::plugin::PluginManifest =
            serde_json::from_value(value["manifest"].clone()).unwrap();
        let pack = manifest.model_pack.unwrap();
        let spec = LocalModelSpec {
            plugin_id: manifest.id,
            manifest_version: manifest.version,
            provider_id: manifest.provider.id,
            engine: pack.engine,
            model_dir: std::env::var("SAYIT_RESIDENCY_MODEL_DIR").unwrap().into(),
            files: pack.files,
            params: pack.params,
        };
        let wave = sherpa_onnx::Wave::read(&std::env::var("SAYIT_RESIDENCY_WAV").unwrap()).unwrap();
        assert_eq!(wave.sample_rate(), 16000);
        let resident = std::env::var("SAYIT_RESIDENCY_MODE").as_deref() == Ok("resident");
        let baseline = crate::performance_test_support::memory().working_set;
        let start = Instant::now();
        if resident {
            let jobs = cache().lock().unwrap().reconcile(vec![spec.clone()]);
            *jobs[0].1.state.lock().unwrap() = LoadState::Ready(Arc::new(load(&spec).unwrap()));
        }
        let preload_ms = start.elapsed().as_secs_f64() * 1000.0;
        let loaded = crate::performance_test_support::memory().working_set;
        let mut times = Vec::new();
        let mut text = String::new();
        for _ in 0..2 {
            let start = Instant::now();
            let mut session =
                super::super::OfflineVadSession::create_realtime(&spec, Default::default())
                    .unwrap();
            let create_ms = start.elapsed().as_secs_f64() * 1000.0;
            let mut segments = session.accept(wave.samples(), None).unwrap();
            segments.extend(session.finish(None).unwrap());
            text = segments
                .iter()
                .map(|segment| segment.text.as_str())
                .collect::<Vec<_>>()
                .join("");
            assert!(
                text.chars().count() >= 3,
                "必须通过宿主 VAD 会话出字：{text}"
            );
            times.push(serde_json::json!({"createMs":create_ms,"resultMs":start.elapsed().as_secs_f64()*1000.0,"workingSetBytes":crate::performance_test_support::memory().working_set}));
        }
        let after = crate::performance_test_support::memory().working_set;
        if resident {
            assert_eq!(status(&spec).unwrap().state, "loaded");
            assert!(cache().lock().unwrap().get(&spec).unwrap().is_some());
        }
        println!(
            "RESIDENCY_PROBE {}",
            serde_json::json!({
                "pluginId":spec.plugin_id,"resident":resident,"baselineBytes":baseline,
                "loadedBytes":loaded,"afterShortDecodeBytes":after,"preloadMs":preload_ms,"sessions":times,"text":text
            })
        );
        if resident {
            let first = super::super::OfflineVadSession::create_realtime(&spec, Default::default())
                .unwrap();
            let second =
                super::super::OfflineVadSession::create_realtime(&spec, Default::default())
                    .unwrap();
            assert!(Arc::ptr_eq(
                sherpa(&first.engine.recognizer),
                sherpa(&second.engine.recognizer)
            ));
            let samples = wave.samples();
            std::thread::scope(|scope| {
                let decode = |mut session: super::super::OfflineVadSession| {
                    let mut segments = session.accept(samples, None).unwrap();
                    segments.extend(session.finish(None).unwrap());
                    segments
                        .into_iter()
                        .map(|segment| segment.text)
                        .collect::<Vec<_>>()
                        .join("")
                };
                let first_task = scope.spawn(move || decode(first));
                let second_task = scope.spawn(move || decode(second));
                assert_eq!(first_task.join().unwrap(), text);
                assert_eq!(second_task.join().unwrap(), text);
            });
            let cached = cache().lock().unwrap().get(&spec).unwrap().unwrap();
            let Recognizer::Segment(realtime_recognizer) = cached.as_ref() else {
                panic!("需要离线模型")
            };
            let realtime_recognizer = sherpa(realtime_recognizer);
            let temporary =
                super::super::OfflineVadSession::create_temporary(&spec, Default::default())
                    .unwrap();
            assert!(!Arc::ptr_eq(
                realtime_recognizer,
                sherpa(&temporary.engine.recognizer)
            ));
            drop(temporary);
            // 实时识别器被占用时，完整文件路径仍应使用独立实例完成识别。
            let held = realtime_recognizer.lock().unwrap();
            let file_spec = spec.clone();
            let path = std::env::var("SAYIT_RESIDENCY_WAV").unwrap();
            let (tx, rx) = std::sync::mpsc::channel();
            let file_task = std::thread::spawn(move || {
                let result = super::super::recognize_audio_file(
                    &file_spec,
                    &path,
                    &Default::default(),
                    None,
                );
                let _ = tx.send(result);
            });
            let result = rx.recv_timeout(std::time::Duration::from_secs(30));
            drop(held);
            file_task.join().unwrap();
            drop(cached);
            let (segments, duration) = result.expect("文件任务不得等待实时识别器锁").unwrap();
            assert!(!segments.is_empty());
            assert!(segments
                .iter()
                .all(|segment| segment.begin_ms < segment.end_ms && segment.end_ms <= duration));
            let mut active =
                super::super::OfflineVadSession::create_realtime(&spec, Default::default())
                    .unwrap();
            let weak = Arc::downgrade(sherpa(&active.engine.recognizer));
            {
                let cached = cache().lock().unwrap().get(&spec).unwrap().unwrap();
                let Recognizer::Segment(recognizer) = cached.as_ref() else {
                    panic!("需要离线模型")
                };
                assert!(Arc::ptr_eq(
                    sherpa(recognizer),
                    sherpa(&active.engine.recognizer)
                ));
            }
            cache().lock().unwrap().reconcile(vec![]);
            assert!(weak.upgrade().is_some());
            let mut output = active.accept(wave.samples(), None).unwrap();
            output.extend(active.finish(None).unwrap());
            assert!(!output.is_empty(), "淘汰期间的活动会话必须仍能识别");
            assert!(weak.upgrade().is_none(), "最后一个会话结束后必须释放识别器");
        }
        cache().lock().unwrap().reconcile(vec![]);
    }
}
