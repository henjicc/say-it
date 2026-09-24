use crate::prelude::*;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioDeviceInfo {
    pub(crate) name: String,
    pub(crate) is_default: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AudioDeviceList {
    pub(crate) inputs: Vec<AudioDeviceInfo>,
    pub(crate) outputs: Vec<AudioDeviceInfo>,
}

#[tauri::command]
pub(crate) fn list_audio_devices() -> Result<AudioDeviceList, String> {
    let host = cpal::default_host();

    let default_input_name = host
        .default_input_device()
        .and_then(|device| device.name().ok());
    let default_output_name = host
        .default_output_device()
        .and_then(|device| device.name().ok());

    let inputs = host
        .input_devices()
        .map_err(|e| format!("枚举麦克风设备失败: {e}"))?
        .filter_map(|device| device.name().ok())
        .map(|name| AudioDeviceInfo {
            is_default: Some(&name) == default_input_name.as_ref(),
            name,
        })
        .collect();

    let outputs = host
        .output_devices()
        .map_err(|e| format!("枚举播放设备失败: {e}"))?
        .filter_map(|device| device.name().ok())
        .map(|name| AudioDeviceInfo {
            is_default: Some(&name) == default_output_name.as_ref(),
            name,
        })
        .collect();

    Ok(AudioDeviceList { inputs, outputs })
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    #[ignore = "本机音频设备枚举性能诊断，不启动采集或播放"]
    fn device_enumeration_profile() {
        use windows::Win32::System::Threading::{GetCurrentProcess, GetProcessHandleCount};
        let mut rounds = Vec::new();
        for cycle in 0..12 {
            let started = std::time::Instant::now();
            let devices = list_audio_devices().unwrap();
            let elapsed_ms = started.elapsed().as_secs_f64() * 1000.0;
            std::thread::sleep(std::time::Duration::from_millis(100));
            let mut handles = 0;
            unsafe {
                GetProcessHandleCount(GetCurrentProcess(), &mut handles).unwrap();
            }
            rounds.push(serde_json::json!({"cycle":cycle,"handles":handles,
                "privateBytes":crate::performance_test_support::memory().private_usage,
                "elapsedMs":elapsed_ms,"inputs":devices.inputs.len(),"outputs":devices.outputs.len()}));
        }
        println!(
            "PERF_RESULT {}",
            serde_json::json!({"scenario":"device-enumeration","rounds":rounds})
        );
    }
}
