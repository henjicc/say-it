//! 录音识别结果缓存：同一个文件用同样的模型、参数、热词和上下文再识别时，直接复用上次的结果，
//! 不再上传、调用模型或产生费用。转写、生成字幕与文稿对齐共用这一份。
//!
//! 文件按路径、大小和修改时间识别，内容变了（重新导出、剪辑过）就当作新文件。取舍：用保留修改时间
//! 的方式复制来一个大小恰好相同的不同文件覆盖原路径时会误用旧结果，概率很低，不为此读整个文件算摘要。
//! 应用、插件或模型包升级后不复用旧结果，识别结果的格式与内容可能已经改进。
//! 结果含识别出的文字，放在数据根目录里，随数据目录迁移、随「重置数据」清空。
use std::path::Path;

use serde_json::json;
use sha2::{Digest, Sha256};

use crate::providers::alibabacloud::{TranscriptionParams, TranscriptionResult};
use crate::providers::RequestCustomization;

pub(crate) const DIR: &str = "transcription-cache";
/// 只留最近用过的这么多份，旧的按修改时间淘汰。
const MAX_ENTRIES: usize = 50;

pub(crate) fn key(
    file_path: &str,
    params: &TranscriptionParams,
    customization: &RequestCustomization,
    implementation_revision: &str,
) -> Result<String, String> {
    let diarization = params.diarization_enabled.unwrap_or(false);
    let path = std::fs::canonicalize(file_path).map_err(|error| format!("读取音视频文件失败：{error}"))?;
    let metadata = std::fs::metadata(&path).map_err(|error| format!("读取音视频文件失败：{error}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |time| time.as_nanos());
    let material = json!({
        "path": path,
        "size": metadata.len(),
        "modified": modified.to_string(),
        "app": env!("CARGO_PKG_VERSION"),
        "implementation": implementation_revision,
        "model": params.model_id(),
        "languageHints": params.language_hints,
        "diarization": diarization,
        "speakerCount": if diarization { params.speaker_count } else { None },
        "channelId": params.channel_id,
        "specialWordFilter": params.special_word_filter,
        "hotwords": customization.hotwords,
        "context": customization.context,
    });
    Ok(format!("{:x}", Sha256::digest(material.to_string().as_bytes())))
}

/// 没有缓存时返回 `None`；读不了或已损坏时记一条诊断并当作没有，照常重新识别。
pub(crate) fn load(dir: &Path, key: &str) -> Option<TranscriptionResult> {
    let path = dir.join(format!("{key}.json"));
    let unreadable = |error: String| {
        crate::application::diagnostics::event(
            "warn",
            "transcription.cacheUnreadable",
            json!({ "key": key, "error": error }),
        );
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            unreadable(error.to_string());
            return None;
        }
    };
    let result = match serde_json::from_slice(&bytes) {
        Ok(result) => result,
        Err(error) => {
            unreadable(error.to_string());
            return None;
        }
    };
    // 刷新修改时间，淘汰时按最近使用排序。
    if let Err(error) = std::fs::File::options()
        .append(true)
        .open(&path)
        .and_then(|file| file.set_modified(std::time::SystemTime::now()))
    {
        unreadable(error.to_string());
    }
    Some(result)
}

/// 什么也没识别出来的结果不存：多半是出了问题，下次应当重新识别。
pub(crate) fn store(dir: &Path, key: &str, result: &TranscriptionResult) -> Result<(), String> {
    if result.transcripts.iter().all(|transcript| transcript.text.trim().is_empty()) {
        return Ok(());
    }
    let bytes = serde_json::to_vec(result).map_err(|error| error.to_string())?;
    let path = dir.join(format!("{key}.json"));
    let temporary = dir.join(format!("{key}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|error| format!("保存识别结果缓存失败：{error}"))?;
    prune(dir)
}

fn prune(dir: &Path) -> Result<(), String> {
    let mut entries = std::fs::read_dir(dir)
        .map_err(|error| format!("读取识别结果缓存失败：{error}"))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .filter_map(|entry| Some((entry.metadata().ok()?.modified().ok()?, entry.path())))
        .collect::<Vec<_>>();
    if entries.len() <= MAX_ENTRIES {
        return Ok(());
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, path) in entries.drain(MAX_ENTRIES..) {
        match std::fs::remove_file(&path) {
            // 同时结束的另一个任务已经删掉了。
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("清理识别结果缓存失败：{error}"));
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("sayit-transcription-cache-{name}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn result(text: &str) -> TranscriptionResult {
        serde_json::from_value(json!({ "transcripts": [{ "text": text, "sentences": [] }] })).unwrap()
    }

    #[test]
    fn same_file_and_settings_share_a_key_and_any_change_makes_a_new_one() {
        let dir = temp_dir("key");
        let file = dir.join("talk.wav");
        std::fs::write(&file, b"audio").unwrap();
        let file = file.to_str().unwrap();
        let params = TranscriptionParams::default();
        let plain = RequestCustomization::default();
        let first = key(file, &params, &plain, "").unwrap();
        let same = TranscriptionParams { diarization_enabled: Some(false), speaker_count: Some(3), ..params.clone() };
        assert_eq!(first, key(file, &same, &plain, "").unwrap(), "不影响结果的写法差异不该让缓存失效");

        let other_model = TranscriptionParams { model: "fun-asr".into(), ..params.clone() };
        assert_ne!(first, key(file, &other_model, &plain, "").unwrap());
        let with_context = RequestCustomization { context: "说吧".into(), ..Default::default() };
        assert_ne!(first, key(file, &params, &with_context, "").unwrap());
        assert_ne!(first, key(file, &params, &plain, "1.0.1").unwrap(), "模型包升级后重新识别");
        std::fs::write(file, b"edited audio").unwrap();
        assert_ne!(first, key(file, &params, &plain, "").unwrap());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stored_result_is_reused_and_old_entries_are_pruned() {
        let dir = temp_dir("store");
        assert!(load(&dir, "missing").is_none());
        store(&dir, "empty", &result("  ")).unwrap();
        assert!(load(&dir, "empty").is_none(), "空结果不缓存");
        let base = std::time::SystemTime::now() - std::time::Duration::from_secs(3600);
        for index in 0..MAX_ENTRIES + 3 {
            let key = format!("k{index}");
            store(&dir, &key, &result(&format!("第{index}份"))).unwrap();
            // 连续写入的修改时间可能相同，显式拉开先后，淘汰顺序才确定。
            std::fs::File::options()
                .append(true)
                .open(dir.join(format!("{key}.json")))
                .unwrap()
                .set_modified(base + std::time::Duration::from_secs(index as u64))
                .unwrap();
        }
        store(&dir, "latest", &result("最新")).unwrap();
        let kept = std::fs::read_dir(&dir).unwrap().count();
        assert_eq!(kept, MAX_ENTRIES);
        assert_eq!(load(&dir, "latest").unwrap().transcripts[0].text, "最新");
        assert!(load(&dir, "k0").is_none(), "最久没用的先淘汰");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
