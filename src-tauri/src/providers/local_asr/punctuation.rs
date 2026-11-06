use sherpa_onnx::{OfflinePunctuation, OfflinePunctuationConfig};

use super::{cache, ensure_ready, int_param, param_path};
use crate::providers::plugin::LocalModelSpec;

pub(super) fn create_punctuator(spec: &LocalModelSpec) -> Result<OfflinePunctuation, String> {
    ensure_ready(spec)?;
    let mut config = OfflinePunctuationConfig::default();
    config.model.ct_transformer = Some(param_path(spec, "model")?);
    config.model.num_threads = int_param(&spec.params, "numThreads", 1);
    OfflinePunctuation::create(&config)
        .ok_or_else(|| "创建 sherpa-onnx 标点模型失败；请检查模型包文件与参数".into())
}

/// 给整段识别文本补标点。模型未常驻时要先校验并加载一次，多等约半秒。
pub fn add_punctuation(spec: &LocalModelSpec, text: &str) -> Result<String, String> {
    let shared = cache::acquire(spec)?;
    let cache::Recognizer::Punctuation(punctuator) = shared.as_ref() else {
        return Err("模型引擎不是标点模型".into());
    };
    let output = punctuator
        .lock()
        .map_err(|_| "标点模型锁失败")?
        .add_punctuation(text)
        .ok_or("标点模型未返回结果")?;
    Ok(halfwidth_in_english(&output))
}

/// 模型对英文也只输出全角标点。标点前是英文字母、后面接着英文字母时换成半角加空格；
/// 整段没有中文时句末标点也换成半角。中英交界和数字之间保持全角。
fn halfwidth_in_english(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let english_only = !chars
        .iter()
        .any(|value| ('\u{4e00}'..='\u{9fff}').contains(value));
    let mut output = String::with_capacity(text.len());
    for (index, &current) in chars.iter().enumerate() {
        let halfwidth = match current {
            '，' => ',',
            '。' => '.',
            '？' => '?',
            '！' => '!',
            _ => {
                output.push(current);
                continue;
            }
        };
        let before = index
            .checked_sub(1)
            .is_some_and(|previous| chars[previous].is_ascii_alphabetic());
        let next = chars[index + 1..].iter().find(|value| **value != ' ');
        let after = match next {
            Some(value) => value.is_ascii_alphabetic(),
            None => english_only,
        };
        if !(before && after) {
            output.push(current);
            continue;
        }
        output.push(halfwidth);
        if next.is_some() && chars.get(index + 1) != Some(&' ') {
            output.push(' ');
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::plugin::ModelPackFileManifest;
    use std::path::PathBuf;
    use std::time::Instant;

    #[test]
    fn uses_halfwidth_only_inside_english() {
        assert_eq!(
            halfwidth_in_english("我很好，thank you，are you ok，谢谢你。"),
            "我很好，thank you, are you ok，谢谢你。"
        );
        assert_eq!(
            halfwidth_in_english("how are you？i am fine。"),
            "how are you? i am fine."
        );
        assert_eq!(halfwidth_in_english("我选了2，3号。"), "我选了2，3号。");
    }

    #[test]
    #[ignore = "需要 SAYIT_PUNCT_POC_DIR 指向官方 CT-Transformer INT8 标点模型目录"]
    fn punctuates_chinese_with_official_ct_transformer() {
        let spec = LocalModelSpec {
            manifest_version: "1.0.0".into(),
            plugin_id: "poc".into(),
            provider_id: "poc".into(),
            engine: "sherpa-onnx-punctuation".into(),
            model_dir: PathBuf::from(std::env::var("SAYIT_PUNCT_POC_DIR").unwrap()),
            files: vec![ModelPackFileManifest {
                path: "model.int8.onnx".into(),
                sha256: "65a3fb9f5ad7bfb96bf69e0dc4481df97f6ee60513c1d94ce981ba6effd524b1".into(),
                size_bytes: 75_519_198,
                download: None,
            }],
            params: serde_json::json!({ "model": "model.int8.onnx" }),
        };
        let started = Instant::now();
        let punctuator = create_punctuator(&spec).unwrap();
        println!("加载耗时 {:?}", started.elapsed());
        for text in [
            "我们都是木头人不会说话不会动",
            "今天下午三点开会你记得把上周的数据整理好发给我",
            "这个功能我觉得还可以但是有几个地方需要再优化一下比如说加载速度和内存占用",
            "你吃饭了吗要不要一起去楼下那家面馆",
            "帮我打开一下设置然后把快捷键改成右边的alt键",
            "我刚才用claude code写了一个脚本效果还不错",
        ] {
            let started = Instant::now();
            let output = punctuator.add_punctuation(text).unwrap();
            println!("{:?} {output}", started.elapsed());
            assert_ne!(output, text);
        }
    }
}
