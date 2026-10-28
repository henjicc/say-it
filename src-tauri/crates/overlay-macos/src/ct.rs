//! 渲染线程内使用的 CoreText 排版；所有字符串范围均为 UTF-16。
use std::cell::RefCell;
use std::collections::HashMap;
use std::ptr::null;
use std::rc::Rc;

use objc2_core_foundation::{
    CFAttributedString, CFBoolean, CFDictionary, CFMutableAttributedString, CFNumber, CFRange,
    CFRetained, CFString, CFType, CGAffineTransform,
};
use objc2_core_graphics::{CGColor, CGContext};
use objc2_core_text::{
    kCTFontAttributeName, kCTFontFamilyNameAttribute, kCTFontTraitsAttribute, kCTFontWeightTrait,
    kCTForegroundColorAttributeName, kCTForegroundColorFromContextAttributeName,
    kCTLanguageAttributeName, CTFont, CTFontDescriptor, CTFontUIFontType, CTLine, CTRun,
    CTTypesetter,
};
use sayit_overlay_core::canvas::{
    FontFamily, ParagraphAlign, TextAlign, TextEngine, TextMetrics, TextStyle, TextTint,
};
use sayit_overlay_core::geom::{Color, Point};

#[derive(Default)]
pub struct CtText {
    // None 为系统界面字体，Some 为请求的族名；回退后的字体仍以请求键缓存。
    fonts: RefCell<HashMap<(Option<String>, u32, u16), CFRetained<CTFont>>>,
    #[cfg(test)]
    font_resolutions: std::cell::Cell<usize>,
}

impl CtText {
    fn font(&self, style: &TextStyle<'_>) -> Option<CFRetained<CTFont>> {
        let family = match style.family {
            FontFamily::SystemUi => None,
            FontFamily::Named(name) => Some(name.to_owned()),
            FontFamily::SymbolGlyphs => return None,
        };
        let key = (family, style.size.to_bits(), style.weight);
        if let Some(font) = self.fonts.borrow().get(&key) {
            return Some(font.clone());
        }
        #[cfg(test)]
        self.font_resolutions.set(self.font_resolutions.get() + 1);
        let font = resolve_font(style)?;
        self.fonts.borrow_mut().insert(key, font.clone());
        Some(font)
    }
}

struct Line {
    native: CFRetained<CTLine>,
    range: CFRange,
    x: f64,
    baseline: f64,
}

pub struct CtLayout {
    attributed: CFRetained<CFAttributedString>,
    lines: Vec<Line>,
    metrics: TextMetrics,
}

fn resolve_font(style: &TextStyle<'_>) -> Option<CFRetained<CTFont>> {
    let weight = CFNumber::new_f64(match style.weight {
        0..=499 => 0.0,
        500..=649 => 0.3,
        _ => 0.4,
    });
    // SAFETY: 全部字典键来自 CoreText，值类型符合这些键的约定。
    unsafe {
        let traits = CFDictionary::<CFString, CFType>::from_slices(
            &[kCTFontWeightTrait],
            &[weight.as_ref()],
        );
        let attributes = CFDictionary::<CFString, CFType>::from_slices(
            &[kCTFontTraitsAttribute],
            &[traits.as_ref()],
        );
        let descriptor = CTFontDescriptor::with_attributes(attributes.as_ref());
        let system = || {
            CTFont::new_ui_font_for_language(
                CTFontUIFontType::System,
                style.size as f64,
                Some(&CFString::from_static_str("zh-Hans")),
            )
            .map(|font| font.copy_with_attributes(style.size as f64, null(), Some(&descriptor)))
        };
        match style.family {
            FontFamily::SymbolGlyphs => None,
            FontFamily::SystemUi => system(),
            FontFamily::Named(name) => {
                for family in [name, "PingFang SC"] {
                    let requested = CFString::from_str(family);
                    let attrs = CFDictionary::<CFString, CFType>::from_slices(
                        &[kCTFontFamilyNameAttribute, kCTFontTraitsAttribute],
                        &[requested.as_ref(), traits.as_ref()],
                    );
                    let descriptor = CTFontDescriptor::with_attributes(attrs.as_ref());
                    let font = CTFont::with_font_descriptor(&descriptor, style.size as f64, null());
                    if font.family_name().to_string().eq_ignore_ascii_case(family) {
                        return Some(font);
                    }
                }
                system()
            }
        }
    }
}

fn attributed(text: &str, font: &CTFont) -> Option<CFRetained<CFAttributedString>> {
    let text = CFString::from_str(text);
    let language = CFString::from_static_str("zh-Hans");
    // SAFETY: 属性字典包含有效的 CTFont、CFBoolean 与 BCP-47 语言字符串。
    unsafe {
        let attrs = CFDictionary::<CFString, CFType>::from_slices(
            &[
                kCTFontAttributeName,
                kCTForegroundColorFromContextAttributeName,
                kCTLanguageAttributeName,
            ],
            &[
                font.as_ref(),
                CFBoolean::new(true).as_ref(),
                language.as_ref(),
            ],
        );
        CFAttributedString::new(None, Some(&text), Some(attrs.as_ref()))
    }
}

impl TextEngine for CtText {
    type Layout = Rc<CtLayout>;

    fn layout(
        &self,
        text: &str,
        style: &TextStyle<'_>,
        max_width: f32,
        max_height: f32,
    ) -> Option<Self::Layout> {
        if ![style.size, max_width, max_height]
            .iter()
            .all(|v| v.is_finite() && *v > 0.0)
            || style.line_spacing.is_some_and(|s| {
                !s.height.is_finite() || s.height <= 0.0 || !s.baseline.is_finite()
            })
        {
            return None;
        }
        let font = self.font(style)?;
        let attributed = attributed(text, &font)?;
        // SAFETY: CoreText 句柄留在本线程，范围不超过属性串的 UTF-16 长度。
        unsafe {
            let setter = CTTypesetter::with_attributed_string(&attributed);
            let total = attributed.length();
            let mut ranges = Vec::new();
            if !style.wrap || total == 0 {
                ranges.push(CFRange {
                    location: 0,
                    length: total,
                });
            } else {
                let units: Vec<_> = text.encode_utf16().collect();
                let mut start = 0;
                while start < total {
                    let suggested = setter.suggest_line_break(start, max_width as f64);
                    // 极窄布局也必须按一个完整 Unicode 标量前进，不能切断代理对。
                    let mut count = if suggested > 0 {
                        suggested.min(total - start)
                    } else if (0xD800..=0xDBFF).contains(&units[start as usize]) {
                        2
                    } else {
                        1
                    };
                    // 强制放入超宽字形时，将紧随其后的硬换行归入同一行；CRLF 是一个换行。
                    // 若本行已以换行结束，下一换行必须保留为空行。
                    let end = (start + count) as usize;
                    let previous = units[end - 1];
                    let tail = &units[end..];
                    if previous == 0x0D && tail.first() == Some(&0x0A) {
                        count += 1;
                    } else if previous != 0x0D && previous != 0x0A {
                        count += match tail {
                            [0x0D, 0x0A, ..] => 2,
                            [0x0D | 0x0A, ..] => 1,
                            _ => 0,
                        };
                    }
                    ranges.push(CFRange {
                        location: start,
                        length: count,
                    });
                    start += count;
                }
                if text.ends_with('\n') || text.ends_with('\r') {
                    ranges.push(CFRange {
                        location: total,
                        length: 0,
                    });
                }
            }
            let mut lines = Vec::with_capacity(ranges.len());
            let mut height = 0.0f64;
            let mut widest = 0.0f64;
            for range in ranges {
                // length=0 在 CTTypesetter 中表示直到串尾；这里只有空串或末尾空行。
                let line = setter.line(range);
                let (mut ascent, mut descent, mut leading) = (0.0, 0.0, 0.0);
                let width = (line.typographic_bounds(&mut ascent, &mut descent, &mut leading)
                    - line.trailing_whitespace_width())
                .max(0.0);
                let (line_height, baseline) = if let Some(spacing) = style.line_spacing {
                    (spacing.height as f64, spacing.baseline as f64)
                } else {
                    if ascent + descent == 0.0 {
                        ascent = font.ascent();
                        descent = font.descent();
                        leading = font.leading();
                    }
                    (ascent + descent + leading, ascent)
                };
                let x = match style.align {
                    TextAlign::Leading => 0.0,
                    TextAlign::Center => (max_width as f64 - width) / 2.0,
                };
                lines.push(Line {
                    native: line,
                    range,
                    x,
                    baseline: height + baseline,
                });
                height += line_height;
                widest = widest.max(width);
            }
            let top = match style.paragraph {
                ParagraphAlign::Near => 0.0,
                ParagraphAlign::Center => (max_height as f64 - height) / 2.0,
            };
            for line in &mut lines {
                line.baseline += top;
            }
            let left = lines
                .iter()
                .map(|line| line.x)
                .fold(f64::INFINITY, f64::min);
            let metrics = TextMetrics {
                left: left as f32,
                top: top as f32,
                width: widest as f32,
                height: height as f32,
                line_count: lines.len() as u32,
            };
            Some(Rc::new(CtLayout {
                attributed,
                lines,
                metrics,
            }))
        }
    }

    fn metrics(&self, layout: &Self::Layout) -> TextMetrics {
        layout.metrics
    }
}

pub(crate) fn set_fill(ctx: &CGContext, color: Color) {
    let color = CGColor::new_srgb(
        color.r as f64,
        color.g as f64,
        color.b as f64,
        color.a as f64,
    );
    CGContext::set_fill_color_with_color(Some(ctx), Some(&color));
}

impl CtLayout {
    pub(crate) fn draw(
        &self,
        ctx: &CGContext,
        origin: Point,
        color: Color,
        tint: Option<TextTint>,
    ) {
        if !origin.x.is_finite() || !origin.y.is_finite() {
            return;
        }
        CGContext::save_g_state(Some(ctx));
        // SAFETY: 布局、属性串与绘图上下文都归当前渲染线程所有；区间先夹到串长。
        unsafe {
            let tint = tint.and_then(|t| {
                let start = (t.utf16_start as isize).min(self.attributed.length());
                let end = (u64::from(t.utf16_start) + u64::from(t.utf16_len))
                    .min(self.attributed.length() as u64) as isize;
                (end > start).then_some((
                    t.color,
                    CFRange {
                        location: start,
                        length: end - start,
                    },
                ))
            });
            let tinted = tint.map(|(_, range)| {
                let attr = CFMutableAttributedString::new_copy(None, 0, Some(&self.attributed))
                    .expect("CoreText tint attribute allocation failed");
                // 颜色属性用于划分 CTRun；实际颜色仍从 CGContext 读取，不改变缓存的布局。
                let marker = CGColor::new_srgb(1.0, 0.0, 0.0, 1.0);
                CFMutableAttributedString::set_attribute(
                    Some(&attr),
                    range,
                    Some(kCTForegroundColorAttributeName),
                    Some(marker.as_ref()),
                );
                CTTypesetter::with_attributed_string(&attr)
            });
            for line in &self.lines {
                CGContext::save_g_state(Some(ctx));
                // CoreText 的字形位置（含组合附加符号）使用 y 向上的文字空间。
                // 以基线为原点翻转 CTM，文字矩阵保持单位阵。
                CGContext::translate_ctm(
                    Some(ctx),
                    origin.x as f64 + line.x,
                    origin.y as f64 + line.baseline,
                );
                CGContext::scale_ctm(Some(ctx), 1.0, -1.0);
                let position = || {
                    CGContext::set_text_matrix(
                        Some(ctx),
                        CGAffineTransform {
                            a: 1.0,
                            b: 0.0,
                            c: 0.0,
                            d: 1.0,
                            tx: 0.0,
                            ty: 0.0,
                        },
                    );
                    CGContext::set_text_position(Some(ctx), 0.0, 0.0);
                };
                if let (Some(setter), Some((tint_color, range))) = (&tinted, tint) {
                    let native = setter.line(line.range);
                    let runs = native.glyph_runs();
                    // CTLineGetGlyphRuns 的数组元素类型由 CoreText 保证为 CTRun。
                    for run in runs.cast_unchecked::<CTRun>().iter() {
                        let run_range = run.string_range();
                        let in_tint = run_range.location < range.location + range.length
                            && range.location < run_range.location + run_range.length;
                        set_fill(ctx, if in_tint { tint_color } else { color });
                        position();
                        run.draw(
                            ctx,
                            CFRange {
                                location: 0,
                                length: 0,
                            },
                        );
                    }
                } else {
                    set_fill(ctx, color);
                    position();
                    line.native.draw(ctx);
                }
                CGContext::restore_g_state(Some(ctx));
            }
        }
        CGContext::restore_g_state(Some(ctx));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sayit_overlay_core::canvas::{Canvas, UniformLineSpacing};
    use sayit_overlay_core::geom::Rect;

    pub(crate) fn style() -> TextStyle<'static> {
        TextStyle {
            family: FontFamily::Named("PingFang SC"),
            size: 20.0,
            weight: 400,
            align: TextAlign::Leading,
            paragraph: ParagraphAlign::Near,
            wrap: true,
            line_spacing: Some(UniformLineSpacing {
                height: 30.0,
                baseline: 24.0,
            }),
        }
    }

    #[test]
    fn font_cache_reuses_requested_key_including_fallback() {
        let _guard = crate::cg::cg_test_lock();
        let text = CtText::default();
        let mut style = style();
        for family in ["PingFang SC", "SayIt Missing Font"] {
            style.family = FontFamily::Named(family);
            let before = text.font_resolutions.get();
            text.layout("第一段", &style, 200.0, 100.0).unwrap();
            assert_eq!(text.font_resolutions.get(), before + 1);
            text.layout("不同文本与宽度", &style, 400.0, 100.0).unwrap();
            let font = text.font(&style).unwrap();
            assert_eq!(text.font_resolutions.get(), before + 1);
            assert_eq!(unsafe { font.family_name() }.to_string(), "PingFang SC");
        }
        style.size += 0.5;
        text.layout("字号", &style, 200.0, 100.0).unwrap();
        assert_eq!(text.font_resolutions.get(), 3);
        style.weight = 600;
        text.layout("字重", &style, 200.0, 100.0).unwrap();
        assert_eq!(text.font_resolutions.get(), 4);
        style.family = FontFamily::SystemUi;
        text.layout("系统字体", &style, 200.0, 100.0).unwrap();
        text.layout("再次排版", &style, 200.0, 100.0).unwrap();
        assert_eq!(text.font_resolutions.get(), 5);
    }

    #[test]
    fn combining_marks_stay_above_base_glyph_in_line_and_tinted_runs() {
        let _guard = crate::cg::cg_test_lock();
        let text = CtText::default();
        let style = TextStyle {
            family: FontFamily::Named("Arial"),
            size: 60.0,
            weight: 400,
            align: TextAlign::Leading,
            paragraph: ParagraphAlign::Near,
            wrap: false,
            line_spacing: Some(UniformLineSpacing {
                height: 100.0,
                baseline: 80.0,
            }),
        };
        for scale in [1, 2] {
            for tint in [false, true] {
                let render = |value: &str| {
                    let mut canvas =
                        crate::cg::CgCanvas::new(192 * scale, 128 * scale, scale as f64).unwrap();
                    let layout = text.layout(value, &style, 192.0, 128.0).unwrap();
                    let color = Color::new(1.0, 0.0, 0.0, 1.0);
                    canvas.draw_text(
                        &layout,
                        Point::new(32.0, 8.0),
                        color,
                        tint.then_some(TextTint {
                            utf16_start: 0,
                            utf16_len: value.encode_utf16().count() as u32,
                            color,
                        }),
                    );
                    let pixels = canvas.pixels();
                    let rows: Vec<u64> = pixels
                        .chunks_exact(192 * scale as usize * 4)
                        .map(|row| row.chunks_exact(4).map(|p| u64::from(p[3])).sum())
                        .collect();
                    let top = rows.iter().position(|mass| *mass > 32).unwrap();
                    let bottom = rows.iter().rposition(|mass| *mass > 32).unwrap() + 1;
                    // 文字绘制结束后，后续图形仍使用画布的左上原点和 DIP 坐标。
                    canvas.fill_rounded_rect(Rect::new(2.0, 2.0, 6.0, 6.0), 0.0, color);
                    let after = canvas.pixels();
                    let index = ((3 * scale * 192 * scale + 3 * scale) * 4) as usize;
                    assert_eq!(&after[index..index + 4], [0, 0, 255, 255]);
                    (top, bottom, rows)
                };
                for (base, accented) in [("x", "x\u{301}"), ("a", "a\u{30c}")] {
                    let (base_top, base_bottom, _) = render(base);
                    let (top, bottom, rows) = render(accented);
                    let tolerance = 2 * scale as usize;
                    assert!(
                        top + tolerance < base_top,
                        "accent must be above base: {accented:?}, scale={scale}, tint={tint}"
                    );
                    assert!(
                        bottom <= base_bottom + tolerance,
                        "accent must not fall below base: {accented:?}, scale={scale}, tint={tint}"
                    );
                    assert!(rows[..base_top].iter().sum::<u64>() > 255 * u64::from(scale * scale));
                }
                let decomposed = render("a\u{30c}");
                let precomposed = render("ǎ");
                let tolerance = 2 * scale as usize;
                assert!(decomposed.0.abs_diff(precomposed.0) <= tolerance);
                assert!(decomposed.1.abs_diff(precomposed.1) <= tolerance);
            }
        }
    }

    #[test]
    fn font_resolution_and_uniform_layout() {
        let _guard = crate::cg::cg_test_lock();
        let mut style = style();
        for family in ["PingFang SC", "Microsoft YaHei", "SayIt Missing Font"] {
            style.family = FontFamily::Named(family);
            assert_eq!(
                unsafe { resolve_font(&style).unwrap().family_name() }.to_string(),
                "PingFang SC"
            );
        }
        let text = CtText::default();
        let layout = text
            .layout("一行\n二行\n三行", &style, 500.0, 200.0)
            .unwrap();
        assert_eq!(layout.metrics.line_count, 3);
        assert_eq!(layout.metrics.height, 90.0);
        assert_eq!(
            layout.lines.iter().map(|l| l.baseline).collect::<Vec<_>>(),
            [24.0, 54.0, 84.0]
        );
        style.wrap = false;
        let layout = text
            .layout("很长的替换字幕文本", &style, 10.0, 50.0)
            .unwrap();
        assert_eq!(layout.metrics.line_count, 1);
        assert!(layout.metrics.width > 10.0);
        style.family = FontFamily::SymbolGlyphs;
        assert!(text.layout("x", &style, 100.0, 50.0).is_none());
    }

    #[test]
    fn metrics_alignment_whitespace_and_empty_lines() {
        let _guard = crate::cg::cg_test_lock();
        let text = CtText::default();
        let mut style = style();
        style.wrap = false;
        let a = text.layout("中文", &style, 200.0, 100.0).unwrap();
        let b = text.layout("中文   ", &style, 200.0, 100.0).unwrap();
        assert!((a.metrics.width - b.metrics.width).abs() < 0.01);
        style.align = TextAlign::Center;
        style.paragraph = ParagraphAlign::Center;
        let c = text.layout("中文", &style, 200.0, 100.0).unwrap();
        assert!((c.metrics.left - (200.0 - a.metrics.width) / 2.0).abs() < 0.01);
        assert_eq!(c.metrics.top, 35.0);
        style.wrap = true;
        assert_eq!(
            text.layout("😀\n", &style, 1.0, 100.0)
                .unwrap()
                .metrics
                .line_count,
            2
        );
        assert_eq!(
            text.layout("😀\r\n", &style, 1.0, 100.0)
                .unwrap()
                .metrics
                .line_count,
            2
        );
        assert_eq!(
            text.layout("😀\n\n", &style, 1.0, 100.0)
                .unwrap()
                .metrics
                .line_count,
            3
        );
        assert_eq!(
            text.layout("\n", &style, 1.0, 100.0)
                .unwrap()
                .metrics
                .line_count,
            2
        );
        assert_eq!(
            text.layout("", &style, 100.0, 100.0)
                .unwrap()
                .metrics
                .line_count,
            1
        );
        assert!(text.layout("x", &style, f32::NAN, 100.0).is_none());
    }
}
