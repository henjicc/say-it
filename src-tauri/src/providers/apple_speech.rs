use serde::{Deserialize, Serialize};

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
}

#[cfg(target_os = "macos")]
fn helper_path() -> std::path::PathBuf {
    let installed = std::env::current_exe().ok().and_then(|path| {
        path.parent()
            .map(|parent| parent.join("sayit-apple-speech"))
    });
    if let Some(path) = installed.filter(|path| path.is_file()) {
        return path;
    }
    let target = match std::env::consts::ARCH {
        "aarch64" => "aarch64-apple-darwin",
        _ => "x86_64-apple-darwin",
    };
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("sayit-apple-speech-{target}"))
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
pub fn status() -> AppleSpeechStatus {
    let helper = helper_path();
    if !helper.is_file() {
        return AppleSpeechStatus {
            message: "缺少 Apple 系统语音识别原生助手，请重新安装应用".into(),
            ..Default::default()
        };
    }
    match std::process::Command::new(helper)
        .args(["--probe", "--locale", ""])
        .output()
    {
        Ok(output) => {
            parse_last_status(&output.stdout).unwrap_or_else(|message| AppleSpeechStatus {
                message,
                ..Default::default()
            })
        }
        Err(error) => AppleSpeechStatus {
            message: format!("启动 Apple 系统语音识别原生助手失败：{error}"),
            ..Default::default()
        },
    }
}

#[cfg(not(target_os = "macos"))]
pub fn status() -> AppleSpeechStatus {
    AppleSpeechStatus {
        message: "Apple 系统本地识别仅支持 macOS".into(),
        ..Default::default()
    }
}

pub fn runtime_available() -> bool {
    status().available
}

#[cfg(target_os = "macos")]
pub async fn prepare() -> Result<AppleSpeechStatus, String> {
    let output = tokio::process::Command::new(helper_path())
        .args(["--prepare", "--locale", ""])
        .output()
        .await
        .map_err(|error| format!("启动 macOS 语音资源准备失败：{error}"))?;
    let status = parse_last_status(&output.stdout)?;
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
                r#"{"available":true,"installed":true,"locale":"zh-CN","backend":"SFSpeechRecognizer","authorization":"notDetermined"}"#,
            )
            .unwrap();
        assert!(status.available);
        assert!(status.installed);
        assert_eq!(status.locale, "zh-CN");
        assert_eq!(status.backend, "SFSpeechRecognizer");
        assert_eq!(status.authorization, "notDetermined");
    }
}
