//! OCR 共享数据结构与内置系统 OCR 引擎。
//!
//! 上提到顶层模块的原因：`providers`（OCR 能力工厂）与 `active_app_context`
//! （场景感知截图流水线）都需要这些类型，而 `providers` 不应反向依赖上层
//! 业务模块。归一化坐标（0~1）保证输出与截图分辨率解耦。

use serde::Serialize;

#[cfg(windows)]
pub(crate) mod windows;

/// 折叠所有空白为单个空格。OCR 行文本与窗口正文抽取共用这一套规则，
/// 保证同一段文字在不同来源下可以按内容去重。
pub(crate) fn normalize_text(value: &str) -> String {
    value
        .split_whitespace()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct NormalizedRegion {
    pub(crate) left: f32,
    pub(crate) top: f32,
    pub(crate) right: f32,
    pub(crate) bottom: f32,
}

impl NormalizedRegion {
    pub(crate) fn clamped(self) -> Self {
        let left = self.left.clamp(0.0, 1.0);
        let top = self.top.clamp(0.0, 1.0);
        let right = self.right.clamp(left, 1.0);
        let bottom = self.bottom.clamp(top, 1.0);
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OcrTextBlock {
    pub(crate) text: String,
    pub(crate) confidence: f32,
    pub(crate) bounds: NormalizedRegion,
}
