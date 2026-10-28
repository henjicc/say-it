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

    pub fn parse_hex_rgb(value: Option<&str>) -> Option<Rgb8> {
        let value = value?.trim();
        let value = value.strip_prefix('#').unwrap_or(value);
        if value.len() != 6 {
            return None;
        }
        Some([
            u8::from_str_radix(&value[0..2], 16).ok()?,
            u8::from_str_radix(&value[2..4], 16).ok()?,
            u8::from_str_radix(&value[4..6], 16).ok()?,
        ])
    }

    pub fn mix_rgb(from: Rgb8, to: Rgb8, amount: f32) -> Rgb8 {
        let mix =
            |from: u8, to: u8| (from as f32 + (to as f32 - from as f32) * amount).round() as u8;
        [
            mix(from[0], to[0]),
            mix(from[1], to[1]),
            mix(from[2], to[2]),
        ]
    }

    pub fn background_rgb(&self) -> Rgb8 {
        if let Some(background) = self.custom_background {
            return background;
        }
        let target = if self.tone == Tone::Light {
            [244, 247, 251]
        } else {
            [7, 10, 16]
        };
        Self::mix_rgb(self.accent, target, 0.98)
    }

    pub fn glass_tint_rgb(&self) -> Rgb8 {
        let light = self.tone == Tone::Light;
        let base = if self.custom_background.is_some() {
            self.background_rgb()
        } else if light {
            [248, 250, 252]
        } else {
            [5, 7, 11]
        };
        Self::mix_rgb(self.accent, base, if light { 0.98 } else { 0.97 })
    }
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
    fn background_tint_follows_accent_or_explicit_background() {
        let theme = Theme {
            accent: [255, 64, 19],
            ..Theme::DEFAULT
        };
        assert_eq!(theme.background_rgb(), [12, 11, 16]);
        assert_eq!(theme.glass_tint_rgb(), [13, 9, 11]);
        assert_eq!(
            Theme {
                custom_background: Some([34, 26, 24]),
                ..Theme::DEFAULT
            }
            .background_rgb(),
            [34, 26, 24]
        );
    }

    #[test]
    fn hex_rgb_keeps_the_existing_theme_format() {
        for value in ["#FF4013", "ff4013", "  #Ff4013  "] {
            assert_eq!(Theme::parse_hex_rgb(Some(value)), Some([255, 64, 19]));
        }
        assert_eq!(Theme::parse_hex_rgb(None), None);
        for value in ["", "#fff", "#11223344", "gg0000", "##112233"] {
            assert_eq!(Theme::parse_hex_rgb(Some(value)), None);
        }
    }

    #[test]
    fn theme_colors_match_legacy_acrylic_for_every_channel_pair() {
        // Acrylic 原公式按 f32 计算再 round；覆盖每种强调色/自定义背景通道组合。
        let legacy_mix = |from: u8, to: u8, amount: f32| {
            (from as f32 + (to as f32 - from as f32) * amount).round() as u8
        };
        for tone in [Tone::Dark, Tone::Light] {
            for accent in 0..=255u8 {
                let light = tone == Tone::Light;
                let target = if light { [244, 247, 251] } else { [7, 10, 16] };
                let base = if light { [248, 250, 252] } else { [5, 7, 11] };
                let amount = if light { 0.98 } else { 0.97 };
                let theme = Theme {
                    tone,
                    accent: [accent; 3],
                    custom_background: None,
                };
                assert_eq!(
                    theme.background_rgb(),
                    target.map(|to| legacy_mix(accent, to, 0.98))
                );
                assert_eq!(
                    theme.glass_tint_rgb(),
                    base.map(|to| legacy_mix(accent, to, amount))
                );
                for background in 0..=255u8 {
                    let theme = Theme {
                        custom_background: Some([background; 3]),
                        ..theme
                    };
                    assert_eq!(theme.background_rgb(), [background; 3]);
                    assert_eq!(
                        theme.glass_tint_rgb(),
                        [legacy_mix(accent, background, amount); 3]
                    );
                }
            }
        }
    }

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
