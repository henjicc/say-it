use serde::{Deserialize, Serialize};

#[cfg(target_os = "macos")]
use std::sync::{Mutex, OnceLock};
#[cfg(target_os = "macos")]
use std::time::{Duration, Instant};

pub const PROVIDER_ID: &str = "apple-speech";
pub const PROVIDER_KIND: &str = "builtin-macos-speech";
pub const PROTOCOL: &str = "builtin-macos-speech";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppleSpeechStatus {
    #[serde(default)]
    pub available: bool,
    #[serde(default)]
    pub installed: bool,
    #[serde(default)]
    pub locale: String,
    #[serde(default)]
    pub backend: String,
    #[serde(default)]
    pub authorization: String,
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub identity_valid: bool,
    #[serde(default)]
    pub bundle_identifier: String,
    #[serde(default)]
    pub usage_description_present: bool,
}

#[cfg(target_os = "macos")]
const STATUS_CACHE_TTL: Duration = Duration::from_secs(2);

#[cfg(target_os = "macos")]
static STATUS_CACHE: OnceLock<Mutex<Option<(Instant, AppleSpeechStatus)>>> = OnceLock::new();

#[cfg(target_os = "macos")]
fn running_from_app_bundle() -> bool {
    std::env::current_exe().is_ok_and(|path| {
        path.ancestors().any(|ancestor| {
            ancestor
                .extension()
                .is_some_and(|extension| extension == "app")
        })
    })
}

#[cfg(target_os = "macos")]
pub(crate) fn development_bundle_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join("SayItAppleSpeech.app")
}

#[cfg(target_os = "macos")]
pub(crate) fn uses_development_bundle() -> bool {
    !running_from_app_bundle()
}

#[cfg(target_os = "macos")]
fn helper_path() -> std::path::PathBuf {
    let installed = std::env::current_exe().ok().and_then(|path| {
        running_from_app_bundle()
            .then(|| {
                path.parent()
                    .map(|parent| parent.join("sayit-apple-speech"))
            })
            .flatten()
    });
    if let Some(path) = installed.filter(|path| path.is_file()) {
        return path;
    }
    development_bundle_path()
        .join("Contents")
        .join("MacOS")
        .join("sayit-apple-speech")
}

#[cfg(target_os = "macos")]
fn parse_last_status(stdout: &[u8]) -> Result<AppleSpeechStatus, String> {
    let text = String::from_utf8_lossy(stdout);
    text.lines()
        .rev()
        .find_map(|line| serde_json::from_str::<AppleSpeechStatus>(line).ok())
        .ok_or_else(|| "Apple 系统语音识别助手没有返回有效状态".to_string())
}

#[cfg(target_os = "macos")]
fn probe_status() -> AppleSpeechStatus {
    let helper = helper_path();
    if !helper.is_file() {
        return AppleSpeechStatus {
            message: "缺少 Apple 系统语音识别原生助手，请重新安装应用".into(),
            ..Default::default()
        };
    }
    let development = uses_development_bundle();
    let arguments = if development {
        vec!["--self-check"]
    } else {
        vec!["--probe", "--locale", ""]
    };
    match std::process::Command::new(helper).args(arguments).output() {
        Ok(output) => parse_last_status(&output.stdout)
            .map(|mut status| {
                if development && status.identity_valid {
                    status.available = true;
                    status.installed = true;
                    status.backend = "DevelopmentBundle".into();
                    status.authorization = "managedAtUse".into();
                }
                status
            })
            .unwrap_or_else(|message| {
                let stderr = String::from_utf8_lossy(&output.stderr);
                let detail = stderr.trim();
                AppleSpeechStatus {
                    message: if !detail.is_empty() {
                        format!("Apple 系统语音识别助手启动失败：{detail}")
                    } else {
                        format!("{message}（进程状态：{}）", output.status)
                    },
                    ..Default::default()
                }
            }),
        Err(error) => AppleSpeechStatus {
            message: format!("启动 Apple 系统语音识别原生助手失败：{error}"),
            ..Default::default()
        },
    }
}

#[cfg(target_os = "macos")]
fn cache() -> &'static Mutex<Option<(Instant, AppleSpeechStatus)>> {
    STATUS_CACHE.get_or_init(|| Mutex::new(None))
}

#[cfg(target_os = "macos")]
fn cache_status(status: AppleSpeechStatus) -> AppleSpeechStatus {
    if let Ok(mut cache) = cache().lock() {
        *cache = Some((Instant::now(), status.clone()));
    }
    status
}

#[cfg(target_os = "macos")]
pub fn refresh_status() -> AppleSpeechStatus {
    cache_status(probe_status())
}

#[cfg(target_os = "macos")]
pub fn status() -> AppleSpeechStatus {
    if let Ok(cache) = cache().lock() {
        if let Some((checked_at, status)) = cache.as_ref() {
            if checked_at.elapsed() < STATUS_CACHE_TTL {
                return status.clone();
            }
        }
    }
    refresh_status()
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> AppleSpeechStatus {
    AppleSpeechStatus {
        message: "Apple 系统本地识别仅支持 macOS".into(),
        ..Default::default()
    }
}

#[cfg(not(target_os = "macos"))]
pub fn refresh_status() -> AppleSpeechStatus {
    status()
}

pub fn runtime_available() -> bool {
    let status = status();
    status.identity_valid && !status.backend.is_empty()
}

#[cfg(target_os = "macos")]
pub async fn prepare() -> Result<AppleSpeechStatus, String> {
    if uses_development_bundle() {
        let status = refresh_status();
        return status
            .available
            .then_some(status)
            .ok_or_else(|| "Apple 开发语音助手不可用".to_string());
    }
    let output = tokio::process::Command::new(helper_path())
        .args(["--prepare", "--locale", ""])
        .output()
        .await
        .map_err(|error| format!("启动 macOS 语音资源准备失败：{error}"))?;
    let status = cache_status(parse_last_status(&output.stdout)?);
    if output.status.success() && status.available && status.installed {
        Ok(status)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let message = if !status.message.trim().is_empty() {
            status.message
        } else if !stderr.trim().is_empty() {
            stderr.trim().to_string()
        } else {
            "macOS 本地语音识别资源准备失败".to_string()
        };
        Err(message)
    }
}

#[cfg(not(target_os = "macos"))]
pub async fn prepare() -> Result<AppleSpeechStatus, String> {
    Err("Apple 系统本地识别仅支持 macOS".into())
}

#[cfg(target_os = "macos")]
pub(crate) fn command(sample_rate: u32) -> tokio::process::Command {
    let mut command = tokio::process::Command::new(helper_path());
    command.args(["--locale", "", "--sample-rate", &sample_rate.to_string()]);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_payload_defaults_optional_fields() {
        let status: AppleSpeechStatus =
            serde_json::from_str(
                r#"{"available":true,"installed":true,"locale":"zh-CN","backend":"SFSpeechRecognizer","authorization":"notDetermined","identityValid":true,"bundleIdentifier":"com.henjicc.sayit","usageDescriptionPresent":true}"#,
            )
            .unwrap();
        assert!(status.available);
        assert!(status.installed);
        assert_eq!(status.locale, "zh-CN");
        assert_eq!(status.backend, "SFSpeechRecognizer");
        assert_eq!(status.authorization, "notDetermined");
        assert!(status.identity_valid);
        assert_eq!(status.bundle_identifier, "com.henjicc.sayit");
        assert!(status.usage_description_present);
    }
}
