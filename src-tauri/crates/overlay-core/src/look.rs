use crate::geom::Color;

pub type Rgb8 = [u8; 3];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Profile {
    /// Windows 现行像素：胶囊/悬浮球颜色 rgb×a、悬浮球 ✓/⚠ 用 Segoe MDL2、悬浮球固定暗色令牌、无玻璃。
    WindowsLegacy,
    /// 按 CSS 与主题：straight alpha、状态图标用 lucide 路径、悬浮球随主题与强调色、可选玻璃。
    Css,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    Dark,
    Light,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    pub tone: Tone,
    pub accent: Rgb8,
    pub custom_background: Option<Rgb8>,
}
impl Theme {
    pub const DEFAULT: Self = Self {
        tone: Tone::Dark,
        accent: [40, 110, 200],
        custom_background: None,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlassMaterial {
    UnderWindow,
    Content,
    Sidebar,
}

/// 与 FloatingOrbSettings 的 glass_* 字段一一对应。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glass {
    pub material: GlassMaterial,
    pub tint_percent: u8,
    pub border_percent: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub profile: Profile,
    pub theme: Theme,
    pub glass: Option<Glass>,
}
impl Look {
    pub const WINDOWS_LEGACY: Self = Self {
        profile: Profile::WindowsLegacy,
        theme: Theme::DEFAULT,
        glass: None,
    };
    pub const MACOS_DEFAULT: Self = Self {
        profile: Profile::Css,
        theme: Theme::DEFAULT,
        glass: None,
    };
    /// 胶囊与悬浮球的 CSS 颜色：WindowsLegacy → Color::rgb_times_alpha（原有双重预乘），Css → Color::new。
    /// 字幕自身已是 straight alpha，直接用 Color::new。
    pub fn color(&self, r: f32, g: f32, b: f32, a: f32) -> Color {
        match self.profile {
            Profile::WindowsLegacy => Color::rgb_times_alpha(r, g, b, a),
            Profile::Css => Color::new(r, g, b, a),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profiles_keep_their_brush_alpha_contract() {
        assert_eq!(
            Look::WINDOWS_LEGACY.color(0.8, 0.4, 0.2, 0.5),
            Color::new(0.4, 0.2, 0.1, 0.5)
        );
        assert_eq!(
            Look::MACOS_DEFAULT.color(0.8, 0.4, 0.2, 0.5),
            Color::new(0.8, 0.4, 0.2, 0.5)
        );
        assert_eq!(
            Look::WINDOWS_LEGACY.color(1.0, 1.0, 1.0, 0.0),
            Color::TRANSPARENT
        );
    }
}
