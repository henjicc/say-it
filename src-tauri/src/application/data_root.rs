//! 数据根目录：所有应用数据（设置、插件、模型等）的统一落位入口。
//!
//! 默认根目录为 `app_local_data_dir()`；用户可迁移到自定义位置，此时在默认目录
//! 保留 `data-root.json` 指针文件。路径在进程启动时解析一次（`OnceLock`），
//! 迁移完成后需要重启才会生效，避免运行中各模块读到新旧混合路径。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Instant;

use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager};

const POINTER_FILE: &str = "data-root.json";

/// 参与迁移的数据条目白名单。指针文件本身与 WebView2 运行时缓存（`EBWebView`）
/// 固定留在默认目录：前者是定位新目录的锚点，后者由 WebView2 独占且无法重定位。
const MIGRATABLE_FILES: &[&str] = &[
    "say-it-state.json",
    "say-it-state.json.bak",
    "trusted-plugin-keys.json",
];
const MIGRATABLE_DIRS: &[&str] = &["plugins", "plugin-data", "plugin-webviews", "cues", "models"];

pub(crate) const MIGRATION_EVENT: &str = "data-root-migration";

static DATA_ROOT: OnceLock<PathBuf> = OnceLock::new();
static MIGRATING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Deserialize, Serialize)]
struct PointerFile {
    root: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DataRootStatus {
    /// 本次进程实际使用中的根目录。
    pub(crate) active_root: String,
    /// 指针文件当前指向的根目录（迁移后与 active 不同，需重启）。
    pub(crate) configured_root: String,
    pub(crate) default_root: String,
    pub(crate) is_custom: bool,
    pub(crate) restart_required: bool,
}

fn default_root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path()
        .app_local_data_dir()
        .map_err(|error| format!("定位应用数据目录失败：{error}"))
}

/// 从指针文件解析生效的根目录；指针缺失、损坏或指向无效路径时回退默认目录。
fn resolve_root_from(default_root: &Path) -> PathBuf {
    let pointer = default_root.join(POINTER_FILE);
    let Ok(text) = std::fs::read_to_string(&pointer) else {
        return default_root.to_path_buf();
    };
    match serde_json::from_str::<PointerFile>(&text) {
        Ok(file) => {
            let root = PathBuf::from(file.root.trim());
            if root.is_absolute() && root.is_dir() {
                root
            } else {
                eprintln!("[data-root] 指针指向的目录无效，回退默认目录：{}", root.display());
                default_root.to_path_buf()
            }
        }
        Err(error) => {
            eprintln!("[data-root] 指针文件损坏，回退默认目录：{error}");
            default_root.to_path_buf()
        }
    }
}

/// 启动时调用一次；后续 `data_root()` 全程返回同一路径。
pub(crate) fn initialize(app: &AppHandle) -> Result<(), String> {
    let root = data_root(app)?;
    if crate::debug_log_enabled() {
        eprintln!("[data-root] 数据根目录：{}", root.display());
    }
    Ok(())
}

/// 所有数据路径的唯一入口：返回本次进程生效的数据根目录（保证已创建）。
pub(crate) fn data_root(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(root) = DATA_ROOT.get() {
        return Ok(root.clone());
    }
    let default = default_root(app)?;
    std::fs::create_dir_all(&default).map_err(|error| format!("创建应用数据目录失败：{error}"))?;
    let resolved = resolve_root_from(&default);
    std::fs::create_dir_all(&resolved).map_err(|error| format!("创建数据根目录失败：{error}"))?;
    let _ = DATA_ROOT.set(resolved);
    Ok(DATA_ROOT.get().expect("data root just set").clone())
}

/// 数据根目录下的子目录（自动创建）。
pub(crate) fn data_subdir(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let dir = data_root(app)?.join(name);
    std::fs::create_dir_all(&dir).map_err(|error| format!("创建数据子目录 {name} 失败：{error}"))?;
    Ok(dir)
}

/// 数据根目录下的文件路径（不创建文件，仅保证根目录存在）。
pub(crate) fn data_file(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    Ok(data_root(app)?.join(name))
}

fn status(app: &AppHandle) -> Result<DataRootStatus, String> {
    let default = default_root(app)?;
    let active = data_root(app)?;
    let configured = resolve_root_from(&default);
    Ok(DataRootStatus {
        active_root: active.display().to_string(),
        configured_root: configured.display().to_string(),
        default_root: default.display().to_string(),
        is_custom: !same_path(&configured, &default),
        restart_required: !same_path(&configured, &active),
    })
}

fn same_path(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

#[tauri::command]
pub(crate) fn get_data_root_status(app: AppHandle) -> Result<DataRootStatus, String> {
    status(&app)
}

#[tauri::command]
pub(crate) fn restart_app(app: AppHandle) {
    app.restart();
}

#[tauri::command]
pub(crate) async fn migrate_data_root(app: AppHandle, target: String) -> Result<DataRootStatus, String> {
    if MIGRATING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err("已有一次数据目录迁移正在进行".into());
    }
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            MIGRATING.store(false, Ordering::Release);
        }
    }
    let _guard = Guard;

    let worker_app = app.clone();
    let result = tauri::async_runtime::spawn_blocking(move || migrate_blocking(&worker_app, &target))
        .await
        .map_err(|error| format!("迁移任务执行失败：{error}"))?;
    match &result {
        Ok(_) => emit_progress(&app, "done", 0, 0, 0, 0, None),
        Err(message) => emit_progress(&app, "failed", 0, 0, 0, 0, Some(message)),
    }
    result?;
    status(&app)
}

fn emit_progress(
    app: &AppHandle,
    phase: &str,
    copied_bytes: u64,
    total_bytes: u64,
    copied_files: u64,
    total_files: u64,
    message: Option<&str>,
) {
    let _ = app.emit(
        MIGRATION_EVENT,
        json!({
            "phase": phase,
            "copiedBytes": copied_bytes,
            "totalBytes": total_bytes,
            "copiedFiles": copied_files,
            "totalFiles": total_files,
            "message": message,
        }),
    );
}

fn migrate_blocking(app: &AppHandle, target: &str) -> Result<(), String> {
    let target = target.trim();
    if target.is_empty() {
        return Err("目标目录不能为空".into());
    }
    let target = PathBuf::from(target);
    if !target.is_absolute() {
        return Err("目标目录必须是绝对路径".into());
    }
    let default = default_root(app)?;
    let current = data_root(app)?;

    std::fs::create_dir_all(&target).map_err(|error| format!("创建目标目录失败：{error}"))?;
    let canonical_target = std::fs::canonicalize(&target)
        .map_err(|error| format!("解析目标目录失败：{error}"))?;
    let canonical_current = std::fs::canonicalize(&current)
        .map_err(|error| format!("解析当前数据目录失败：{error}"))?;
    let canonical_default = std::fs::canonicalize(&default).unwrap_or_else(|_| default.clone());

    if canonical_target == canonical_current {
        return Err("目标目录与当前数据目录相同，无需迁移".into());
    }
    if canonical_target.starts_with(&canonical_current) {
        return Err("目标目录不能位于当前数据目录内部".into());
    }
    let target_is_default = canonical_target == canonical_default;
    validate_target_contents(&target, target_is_default)?;
    probe_writable(&target)?;

    let plan = collect_migration_plan(&current)?;
    ensure_free_space(&target, plan.total_bytes)?;

    let app_for_progress = app.clone();
    let mut last_emit = Instant::now();
    let total_bytes = plan.total_bytes;
    let total_files = plan.files.len() as u64;
    let mut progress = move |copied_bytes: u64, copied_files: u64, force: bool| {
        if force || last_emit.elapsed().as_millis() >= 100 {
            last_emit = Instant::now();
            emit_progress(
                &app_for_progress,
                "copying",
                copied_bytes,
                total_bytes,
                copied_files,
                total_files,
                None,
            );
        }
    };

    if let Err(error) = copy_and_verify(&current, &target, &plan, &mut progress) {
        cleanup_partial_target(&target);
        return Err(error);
    }

    // 复制与校验都成功后才切换指针；指针始终写在默认目录。
    if target_is_default {
        remove_pointer(&default)?;
    } else {
        write_pointer(&default, &target)?;
    }

    // 旧数据删除失败不回滚迁移（指针已切换），仅提示手动清理。
    if let Err(warning) = remove_migratable_entries(&current) {
        eprintln!("[data-root] 旧数据清理未完全成功：{warning}");
        return Err(format!(
            "迁移已完成并已切换到新目录，但旧数据清理未完全成功，可手动删除旧目录残留：{warning}"
        ));
    }
    Ok(())
}

struct MigrationPlan {
    /// 相对数据根目录的文件相对路径列表。
    files: Vec<PathBuf>,
    total_bytes: u64,
}

fn collect_migration_plan(source_root: &Path) -> Result<MigrationPlan, String> {
    let mut files = Vec::new();
    let mut total_bytes: u64 = 0;
    for name in MIGRATABLE_FILES {
        let path = source_root.join(name);
        if path.is_file() {
            total_bytes += file_size(&path)?;
            files.push(PathBuf::from(name));
        }
    }
    for name in MIGRATABLE_DIRS {
        let dir = source_root.join(name);
        if dir.is_dir() {
            collect_files_recursive(source_root, &dir, &mut files, &mut total_bytes)?;
        }
    }
    Ok(MigrationPlan { files, total_bytes })
}

fn collect_files_recursive(
    root: &Path,
    dir: &Path,
    files: &mut Vec<PathBuf>,
    total_bytes: &mut u64,
) -> Result<(), String> {
    let entries = std::fs::read_dir(dir).map_err(|error| format!("读取目录 {} 失败：{error}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("遍历目录 {} 失败：{error}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            collect_files_recursive(root, &path, files, total_bytes)?;
        } else if path.is_file() {
            *total_bytes += file_size(&path)?;
            let relative = path
                .strip_prefix(root)
                .map_err(|_| format!("文件 {} 不在数据目录内", path.display()))?;
            files.push(relative.to_path_buf());
        }
    }
    Ok(())
}

fn file_size(path: &Path) -> Result<u64, String> {
    std::fs::metadata(path)
        .map(|meta| meta.len())
        .map_err(|error| format!("读取文件信息 {} 失败：{error}", path.display()))
}

fn validate_target_contents(target: &Path, target_is_default: bool) -> Result<(), String> {
    if target_is_default {
        // 迁回默认目录：默认目录里允许存在指针文件与 WebView2 缓存，
        // 但不允许已有数据条目，避免覆盖来历不明的文件。
        for name in MIGRATABLE_FILES.iter().chain(MIGRATABLE_DIRS.iter()) {
            if target.join(name).exists() {
                return Err(format!(
                    "默认目录中已存在 {name}，请先手动清理后再迁回默认位置"
                ));
            }
        }
        return Ok(());
    }
    let mut entries = std::fs::read_dir(target).map_err(|error| format!("读取目标目录失败：{error}"))?;
    if entries.next().is_some() {
        return Err("目标目录必须为空目录".into());
    }
    Ok(())
}

fn probe_writable(target: &Path) -> Result<(), String> {
    let probe = target.join(format!(".sayit-write-probe-{}", std::process::id()));
    std::fs::write(&probe, b"probe").map_err(|error| format!("目标目录不可写：{error}"))?;
    std::fs::remove_file(&probe).map_err(|error| format!("清理写入探针失败：{error}"))?;
    Ok(())
}

#[cfg(windows)]
fn free_space_bytes(path: &Path) -> Option<u64> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let wide = HSTRING::from(path.as_os_str());
    let mut free: u64 = 0;
    unsafe {
        GetDiskFreeSpaceExW(
            windows::core::PCWSTR(wide.as_ptr()),
            Some(&mut free),
            None,
            None,
        )
        .ok()
        .map(|_| free)
    }
}

#[cfg(not(windows))]
fn free_space_bytes(_path: &Path) -> Option<u64> {
    None
}

fn ensure_free_space(target: &Path, required_bytes: u64) -> Result<(), String> {
    const MARGIN_BYTES: u64 = 64 * 1024 * 1024;
    if let Some(free) = free_space_bytes(target) {
        if free < required_bytes.saturating_add(MARGIN_BYTES) {
            return Err(format!(
                "目标磁盘剩余空间不足：需要约 {} MiB，可用 {} MiB",
                (required_bytes + MARGIN_BYTES) / 1024 / 1024,
                free / 1024 / 1024
            ));
        }
    }
    Ok(())
}

fn copy_and_verify(
    source_root: &Path,
    target_root: &Path,
    plan: &MigrationPlan,
    progress: &mut impl FnMut(u64, u64, bool),
) -> Result<(), String> {
    let mut copied_bytes: u64 = 0;
    let mut copied_files: u64 = 0;
    for relative in &plan.files {
        let from = source_root.join(relative);
        let to = target_root.join(relative);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("创建目录 {} 失败：{error}", parent.display()))?;
        }
        let bytes = std::fs::copy(&from, &to)
            .map_err(|error| format!("复制 {} 失败：{error}", relative.display()))?;
        copied_bytes += bytes;
        copied_files += 1;
        progress(copied_bytes, copied_files, false);
    }
    progress(copied_bytes, copied_files, true);

    // 校验：逐文件核对目标端存在性与字节数（以复制时源端大小为准）。
    let mut verified_bytes: u64 = 0;
    for relative in &plan.files {
        let to = target_root.join(relative);
        if !to.is_file() {
            return Err(format!("校验失败：目标缺少文件 {}", relative.display()));
        }
        verified_bytes += file_size(&to)?;
    }
    if verified_bytes != copied_bytes {
        return Err(format!(
            "校验失败：目标字节数 {verified_bytes} 与已复制字节数 {copied_bytes} 不一致"
        ));
    }
    Ok(())
}

fn cleanup_partial_target(target: &Path) {
    for name in MIGRATABLE_FILES {
        let path = target.join(name);
        if path.is_file() {
            let _ = std::fs::remove_file(path);
        }
    }
    for name in MIGRATABLE_DIRS {
        let path = target.join(name);
        if path.is_dir() {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

fn write_pointer(default_root: &Path, target: &Path) -> Result<(), String> {
    std::fs::create_dir_all(default_root).map_err(|error| format!("创建默认数据目录失败：{error}"))?;
    let pointer = default_root.join(POINTER_FILE);
    let temp = pointer.with_extension(format!("json.tmp-{}", std::process::id()));
    let bytes = serde_json::to_vec_pretty(&PointerFile {
        root: target.display().to_string(),
    })
    .map_err(|error| error.to_string())?;
    std::fs::write(&temp, bytes).map_err(|error| format!("写入指针文件失败：{error}"))?;
    if pointer.exists() {
        std::fs::remove_file(&pointer).map_err(|error| format!("替换指针文件失败：{error}"))?;
    }
    std::fs::rename(&temp, &pointer).map_err(|error| format!("提交指针文件失败：{error}"))
}

fn remove_pointer(default_root: &Path) -> Result<(), String> {
    let pointer = default_root.join(POINTER_FILE);
    if pointer.exists() {
        std::fs::remove_file(&pointer).map_err(|error| format!("移除指针文件失败：{error}"))?;
    }
    Ok(())
}

fn remove_migratable_entries(root: &Path) -> Result<(), String> {
    let mut failures = Vec::new();
    for name in MIGRATABLE_FILES {
        let path = root.join(name);
        if path.is_file() {
            if let Err(error) = std::fs::remove_file(&path) {
                failures.push(format!("{name}: {error}"));
            }
        }
    }
    for name in MIGRATABLE_DIRS {
        let path = root.join(name);
        if path.is_dir() {
            if let Err(error) = std::fs::remove_dir_all(&path) {
                failures.push(format!("{name}: {error}"));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("；"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "say-it-data-root-{tag}-{}-{}",
            std::process::id(),
            Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    use uuid::Uuid;

    #[test]
    fn missing_pointer_falls_back_to_default() {
        let default = temp_dir("missing");
        assert_eq!(resolve_root_from(&default), default);
        std::fs::remove_dir_all(default).unwrap();
    }

    #[test]
    fn corrupt_pointer_falls_back_to_default() {
        let default = temp_dir("corrupt");
        std::fs::write(default.join(POINTER_FILE), b"not-json").unwrap();
        assert_eq!(resolve_root_from(&default), default);
        std::fs::remove_dir_all(default).unwrap();
    }

    #[test]
    fn pointer_to_nonexistent_dir_falls_back_to_default() {
        let default = temp_dir("dangling");
        let gone = default.join("gone-away");
        std::fs::write(
            default.join(POINTER_FILE),
            serde_json::to_vec(&PointerFile { root: gone.display().to_string() }).unwrap(),
        )
        .unwrap();
        assert_eq!(resolve_root_from(&default), default);
        std::fs::remove_dir_all(default).unwrap();
    }

    #[test]
    fn valid_pointer_resolves_to_custom_root() {
        let default = temp_dir("valid");
        let custom = temp_dir("valid-custom");
        std::fs::write(
            default.join(POINTER_FILE),
            serde_json::to_vec(&PointerFile { root: custom.display().to_string() }).unwrap(),
        )
        .unwrap();
        assert_eq!(resolve_root_from(&default), custom);
        std::fs::remove_dir_all(default).unwrap();
        std::fs::remove_dir_all(custom).unwrap();
    }

    fn seed_source(root: &Path) {
        std::fs::write(root.join("say-it-state.json"), b"{\"a\":1}").unwrap();
        std::fs::create_dir_all(root.join("plugins/demo")).unwrap();
        std::fs::write(root.join("plugins/demo/manifest.json"), b"{}").unwrap();
        std::fs::create_dir_all(root.join("cues")).unwrap();
        std::fs::write(root.join("cues/start.audio"), vec![7u8; 128]).unwrap();
        // 白名单之外的内容不应被迁移或删除。
        std::fs::write(root.join(POINTER_FILE), b"{\"root\":\"x\"}").unwrap();
        std::fs::create_dir_all(root.join("EBWebView")).unwrap();
        std::fs::write(root.join("EBWebView/cache.bin"), b"cache").unwrap();
    }

    #[test]
    fn copy_verify_and_cleanup_moves_whitelist_only() {
        let source = temp_dir("copy-src");
        let target = temp_dir("copy-dst");
        seed_source(&source);

        let plan = collect_migration_plan(&source).unwrap();
        assert_eq!(plan.files.len(), 3);
        copy_and_verify(&source, &target, &plan, &mut |_, _, _| {}).unwrap();
        assert!(target.join("say-it-state.json").is_file());
        assert!(target.join("plugins/demo/manifest.json").is_file());
        assert!(target.join("cues/start.audio").is_file());
        assert!(!target.join(POINTER_FILE).exists());
        assert!(!target.join("EBWebView").exists());

        remove_migratable_entries(&source).unwrap();
        assert!(!source.join("say-it-state.json").exists());
        assert!(!source.join("plugins").exists());
        assert!(source.join(POINTER_FILE).exists());
        assert!(source.join("EBWebView/cache.bin").is_file());

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn failed_copy_cleans_partial_target_and_keeps_source() {
        let source = temp_dir("fail-src");
        let target = temp_dir("fail-dst");
        seed_source(&source);
        // 让 cues/start.audio 的目标位置被同名目录占用，制造复制失败。
        std::fs::create_dir_all(target.join("cues/start.audio")).unwrap();

        let plan = collect_migration_plan(&source).unwrap();
        let result = copy_and_verify(&source, &target, &plan, &mut |_, _, _| {});
        assert!(result.is_err());
        cleanup_partial_target(&target);
        assert!(!target.join("say-it-state.json").exists());
        assert!(!target.join("plugins").exists());
        assert!(!target.join("cues").exists());
        assert!(source.join("say-it-state.json").is_file());
        assert!(source.join("plugins/demo/manifest.json").is_file());

        std::fs::remove_dir_all(source).unwrap();
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn pointer_write_and_remove_round_trip() {
        let default = temp_dir("pointer");
        let custom = temp_dir("pointer-custom");
        write_pointer(&default, &custom).unwrap();
        assert_eq!(resolve_root_from(&default), custom);
        remove_pointer(&default).unwrap();
        assert_eq!(resolve_root_from(&default), default);
        std::fs::remove_dir_all(default).unwrap();
        std::fs::remove_dir_all(custom).unwrap();
    }

    #[test]
    fn non_default_target_must_be_empty() {
        let target = temp_dir("nonempty");
        std::fs::write(target.join("existing.txt"), b"x").unwrap();
        assert!(validate_target_contents(&target, false).is_err());
        std::fs::remove_dir_all(target).unwrap();
    }

    #[test]
    fn default_target_rejects_existing_data_entries() {
        let target = temp_dir("default-conflict");
        std::fs::write(target.join(POINTER_FILE), b"{}").unwrap();
        assert!(validate_target_contents(&target, true).is_ok());
        std::fs::create_dir_all(target.join("plugins")).unwrap();
        assert!(validate_target_contents(&target, true).is_err());
        std::fs::remove_dir_all(target).unwrap();
    }
}
