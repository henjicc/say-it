/// 字幕配置，与 sync_presentation 发出的 dictation-indicator-config.subtitle 同构。
/// 字段兜底值与 IndicatorApp.tsx 的默认值一致。
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SubtitleConfig {
    pub display_mode: String,
    pub font_family: String,
    pub font_size: f64,
    pub line_count: u32,
    pub text_color: String,
    pub background_color: String,
    pub rounded: f64,
    pub width: f64,
    pub window_width: f64,
    pub window_height: f64,
    pub anchor: String,
    pub offset_y: f64,
    pub motion_enabled: bool,
    pub motion_duration_ms: u32,
    pub motion_easing: String,
    pub fade_enabled: bool,
    pub fade_duration_ms: u32,
    pub fade_easing: String,
    pub translation_enabled: bool,
    pub translation_layout: String,
    pub translation_order: String,
}

impl Default for SubtitleConfig {
    fn default() -> Self {
        Self {
            display_mode: "scroll".into(),
            font_family: "Microsoft YaHei".into(),
            font_size: 28.0,
            line_count: 2,
            text_color: "#fff".into(),
            background_color: "rgba(5, 7, 10, 0.72)".into(),
            rounded: 18.0,
            width: 880.0,
            window_width: 880.0,
            window_height: 134.0,
            anchor: "bottom".into(),
            offset_y: 36.0,
            motion_enabled: true,
            motion_duration_ms: 120,
            motion_easing: "ease-out".into(),
            fade_enabled: true,
            fade_duration_ms: 180,
            fade_easing: "ease-out".into(),
            translation_enabled: false,
            translation_layout: "bilingual".into(),
            translation_order: "translationFirst".into(),
        }
    }
}

/// CSS 只给 ≤10 字符的短追加挂 .fresh 淡入；长追加直接显示。
pub const FRESH_FADE_MAX_CHARS: usize = 10;

/// 渲染文本上限，与 IndicatorApp.tsx 的 MAX_RENDER_CHARS 一致。
pub const MAX_RENDER_CHARS: usize = 20000;

/// useTextTrack 的末端重叠搜索范围（OVERLAP_SEARCH_MAX）。
const OVERLAP_SEARCH_MAX: usize = 200;

/// 解析 #rgb / #rrggbb / rgb(r,g,b) / rgba(r,g,b,a)（a 为 0..1 浮点）。
pub fn parse_css_color(value: &str) -> Option<(u8, u8, u8, f32)> {
    let value = value.trim();
    if let Some(hex) = value.strip_prefix('#') {
        let [r, g, b] = crate::look::parse_hex_rgb(hex)?;
        return Some((r, g, b, 1.0));
    }
    let (rgba_mode, inner) = if let Some(inner) = value
        .strip_prefix("rgba(")
        .and_then(|v| v.strip_suffix(')'))
    {
        (true, inner)
    } else if let Some(inner) = value.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
        (false, inner)
    } else {
        return None;
    };
    let parts: Vec<&str> = inner.split(',').map(str::trim).collect();
    if parts.len() < 3 || (rgba_mode && parts.len() < 4) {
        return None;
    }
    let channel = |index: usize| parts.get(index)?.trim().parse::<f64>().ok();
    let alpha = if rgba_mode { channel(3)? as f32 } else { 1.0 };
    Some((
        channel(0)?.round().clamp(0.0, 255.0) as u8,
        channel(1)?.round().clamp(0.0, 255.0) as u8,
        channel(2)?.round().clamp(0.0, 255.0) as u8,
        alpha.clamp(0.0, 1.0),
    ))
}

/// useTextTrack 的 stable 前缀 + 末端重叠搜索。JS 按 UTF-16 码元切片；
/// 这里按 char 遍历，BMP 文本完全一致，astral 字符不会落到半个码元上。
/// 返回值是 next 中「稳定前缀」的字符数。
pub fn overlap_chars(prev: &str, next: &str) -> usize {
    let prev: Vec<char> = prev.chars().collect();
    let next: Vec<char> = next.chars().collect();
    let min = prev.len().min(next.len());
    let mut prefix = 0;
    while prefix < min && prev[prefix] == next[prefix] {
        prefix += 1;
    }
    let mut overlap = prefix;
    let floor = prefix.max(min.saturating_sub(OVERLAP_SEARCH_MAX));
    let mut k = min;
    while k > floor {
        // prev.endsWith(next[..k])
        if prev[prev.len() - k..] == next[..k] {
            overlap = k;
            break;
        }
        k -= 1;
    }
    overlap
}

/// nextText.slice(-MAX_RENDER_CHARS).replace(/^\s+/, "") 的 char 安全版本。
pub fn trim_render_text(text: &str) -> String {
    let count = text.chars().count();
    if count <= MAX_RENDER_CHARS {
        return text.to_string();
    }
    text.chars()
        .skip(count - MAX_RENDER_CHARS)
        .collect::<String>()
        .trim_start()
        .to_string()
}

/// 滚动累积模式：内容底对齐，超高部分整体上移（translateY(-overflow)）。
pub fn scroll_offset_y(content_height: f32, flow_height: f32) -> f32 {
    (content_height - flow_height).max(0.0)
}

/// 单句替换模式：内容不宽于框时居中，超宽时贴右显示最新内容（translateX 左移）。
pub fn replace_offset_x(content_width: f32, flow_width: f32) -> f32 {
    let overflow = content_width - flow_width;
    if overflow > 0.0 {
        -overflow
    } else {
        (flow_width - content_width) / 2.0
    }
}

/// CSS ease-* 的三次贝塞尔曲线；按时间 x 反解曲线参数后再取 y。
pub fn ease_value(name: &str, t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if name == "linear" || t == 0.0 || t == 1.0 {
        return t;
    }
    let (x1, x2) = match name {
        "ease-in" => (0.42, 1.0),
        "ease-in-out" => (0.42, 0.58),
        _ => (0.0, 0.58),
    };
    let (mut low, mut high) = (0.0, 1.0);
    for _ in 0..20 {
        let u = (low + high) * 0.5;
        let x = 3.0 * (1.0 - u) * (1.0 - u) * u * x1 + 3.0 * (1.0 - u) * u * u * x2 + u * u * u;
        if x < t {
            low = u;
        } else {
            high = u;
        }
    }
    let u = (low + high) * 0.5;
    3.0 * (1.0 - u) * u * u + u * u * u
}

/// 行高，与 IndicatorApp 的 Math.round(fontSize * 1.38) 一致。
pub fn line_height(font_size: f64) -> f32 {
    (font_size * 1.38).round() as f32
}

/// 内容块高度：原文 lineHeight*lines+28、译文 +20（indicator.css 的 padding 差）。
pub fn block_height(line_height: f32, lines: u32, translation: bool) -> f32 {
    line_height * lines.max(1) as f32 + if translation { 20.0 } else { 28.0 }
}

/// 底部对齐堆叠（#wrap justify-end，gap 10）：输入自上而下各块高度，
/// 返回各块顶部 y。
pub fn stack_from_bottom(window_height: f32, heights: &[f32], gap: f32) -> Vec<f32> {
    let total: f32 = heights.iter().sum::<f32>() + gap * heights.len().saturating_sub(1) as f32;
    let mut y = window_height - total;
    heights
        .iter()
        .map(|height| {
            let top = y;
            y += height + gap;
            top
        })
        .collect()
}

/// 窗口定位，与 indicator.rs fallback_indicator_position 同一数学：
/// 水平居中，anchor 决定垂直位置，margin 为 offsetY（物理像素）。
pub fn window_position(
    area_x: i32,
    area_y: i32,
    area_w: i32,
    area_h: i32,
    win_w: i32,
    win_h: i32,
    anchor: &str,
    margin: i32,
) -> (i32, i32) {
    let x = area_x + (area_w - win_w) / 2;
    let y = match anchor {
        "top" => area_y + margin,
        "center" => area_y + (area_h - win_h) / 2 + margin,
        _ => area_y + area_h - win_h - margin,
    };
    (x, y)
}

/// 字幕交给主应用的动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubtitleAction {
    Close,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn css_color_parses_hex_and_rgba() {
        assert_eq!(parse_css_color("#fff"), Some((255, 255, 255, 1.0)));
        assert_eq!(parse_css_color("#286ec8"), Some((40, 110, 200, 1.0)));
        assert_eq!(
            parse_css_color("rgba(5, 7, 10, 0.72)"),
            Some((5, 7, 10, 0.72))
        );
        assert_eq!(parse_css_color("rgb(1, 2, 3)"), Some((1, 2, 3, 1.0)));
        assert_eq!(parse_css_color("  #FF4013  "), Some((255, 64, 19, 1.0)));
        assert_eq!(parse_css_color("not-a-color"), None);
        assert_eq!(parse_css_color("#12345"), None);
    }

    #[test]
    fn css_hex_color_rejects_non_ascii_and_non_hex_input() {
        for value in [
            "#中abc",
            "#中",
            "#中中",
            "#ａｂｃ",
            "",
            "#",
            "#+12",
            "#+11223",
        ] {
            assert_eq!(parse_css_color(value), None, "{value:?}");
        }
    }

    #[test]
    fn overlap_finds_stable_prefix() {
        assert_eq!(overlap_chars("你好", "你好世"), 2);
        assert_eq!(overlap_chars("", "你好"), 0);
        // 修订：前缀停在第一个不同字符，末端重叠搜索救不回就按前缀算。
        assert_eq!(overlap_chars("你好世界", "你好中国"), 2);
        // 替换模式的整句衔接：新句开头是旧句结尾时按末端重叠。
        assert_eq!(overlap_chars("abcdef", "cdefgh"), 4);
    }

    #[test]
    fn trim_limits_render_text_from_the_tail() {
        let short = "短文本";
        assert_eq!(trim_render_text(short), short);
        let long = "字".repeat(MAX_RENDER_CHARS + 10);
        let trimmed = trim_render_text(&long);
        assert_eq!(trimmed.chars().count(), MAX_RENDER_CHARS);
        let padded = format!("{}\n   ", "字".repeat(MAX_RENDER_CHARS));
        assert!(!trim_render_text(&padded).starts_with(char::is_whitespace));
    }

    #[test]
    fn scroll_and_replace_offsets_match_the_webview_math() {
        assert_eq!(scroll_offset_y(100.0, 78.0), 22.0);
        assert_eq!(scroll_offset_y(50.0, 78.0), 0.0);
        // 未超宽居中，超宽贴右。
        assert_eq!(replace_offset_x(100.0, 400.0), 150.0);
        assert_eq!(replace_offset_x(500.0, 400.0), -100.0);
    }

    #[test]
    fn block_layout_matches_indicator_css() {
        assert_eq!(line_height(28.0), 39.0);
        assert_eq!(block_height(39.0, 2, false), 106.0);
        assert_eq!(block_height(39.0, 2, true), 98.0);
        // 底部对齐堆叠：块间 gap 10，整体贴窗口底。
        let tops = stack_from_bottom(214.0, &[106.0, 98.0], 10.0);
        assert_eq!(tops, vec![0.0, 116.0]);
        let single = stack_from_bottom(134.0, &[106.0], 10.0);
        assert_eq!(single, vec![28.0]);
    }

    #[test]
    fn window_position_matches_fallback_indicator_position() {
        // 与 indicator.rs 的单测数值一致。
        assert_eq!(
            window_position(-1920, 0, 1920, 1080, 460, 188, "bottom", 36),
            (-1190, 856)
        );
        assert_eq!(
            window_position(200, -900, 1600, 900, 400, 180, "top", 24),
            (800, -876)
        );
        assert_eq!(
            window_position(200, -900, 1600, 900, 400, 180, "center", 24),
            (800, -516)
        );
    }

    #[test]
    fn easing_covers_the_css_keyword_set() {
        assert_eq!(ease_value("linear", 0.5), 0.5);
        assert_eq!(ease_value("ease-in", 0.0), 0.0);
        assert_eq!(ease_value("ease-out", 1.0), 1.0);
        assert!((ease_value("ease-in-out", 0.5) - 0.5).abs() < 0.00001);
        assert!((ease_value("ease-out", 0.5) - 0.684643).abs() < 0.00001);
        assert!((ease_value("ease-in", 0.5) - 0.315357).abs() < 0.00001);
        assert_eq!(ease_value("unknown-keyword", 1.0), 1.0);
        assert!(ease_value("ease-out", 0.5) > 0.5);
    }

    #[test]
    fn config_deserializes_the_sync_presentation_payload() {
        let value = serde_json::json!({
            "displayMode": "replace", "fontFamily": "MiSans", "fontSize": 32.0,
            "lineCount": 1, "textColor": "#ffffff",
            "backgroundColor": "rgba(5, 7, 10, 0.72)",
            "rounded": 18, "width": 880.0, "windowWidth": 880.0, "windowHeight": 67.0,
            "anchor": "bottom", "offsetY": 64.0,
            "motionEnabled": false, "motionDurationMs": 120, "motionEasing": "ease-out",
            "fadeEnabled": true, "fadeDurationMs": 180, "fadeEasing": "linear",
            "translationEnabled": true, "translationLayout": "bilingual",
            "translationOrder": "translationFirst"
        });
        let config: SubtitleConfig = serde_json::from_value(value).unwrap();
        assert_eq!(config.display_mode, "replace");
        assert_eq!(config.font_size, 32.0);
        assert_eq!(config.line_count, 1);
        assert!(!config.motion_enabled);
        assert_eq!(config.translation_order, "translationFirst");
        // 缺字段时用与 IndicatorApp 一致的默认值。
        let sparse: SubtitleConfig = serde_json::from_value(serde_json::json!({})).unwrap();
        assert_eq!(sparse.font_size, 28.0);
        assert_eq!(sparse.line_count, 2);
        assert!(sparse.motion_enabled);
        assert!(sparse.fade_enabled);
    }
}
