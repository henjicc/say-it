use crate::geom::{Color, Point, Rect, Stroke, Transform};
use crate::svg::SvgSeg;

/// 编译期常量 SVG 图标；后端按 (key, 填充/描边) 缓存平台路径。
#[derive(Clone, Copy, Debug)]
pub struct SvgIcon {
    pub key: &'static str,
    pub d: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FontFamily<'a> {
    /// 系统界面字体。Windows："Microsoft YaHei UI"；macOS：CTFontCreateUIFontForLanguage(System, "zh-Hans")。
    SystemUi,
    /// 状态字形字体。Windows："Segoe MDL2 Assets"；macOS 无此字体，layout 返回 None
    /// （Profile::Css 下模型改画路径图标，不会请求它）。
    SymbolGlyphs,
    /// 用户配置的族名（字幕）。找不到时 Windows 由 DirectWrite 回退；macOS 依次回退
    /// PingFang SC → 系统界面字体，并以 zh-Hans 作语言属性。
    Named(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TextAlign {
    Leading,
    Center,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ParagraphAlign {
    Near,
    Center,
}

/// 等同 DWRITE_LINE_SPACING_METHOD_UNIFORM：每行高 height，基线距行顶 baseline。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct UniformLineSpacing {
    pub height: f32,
    pub baseline: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle<'a> {
    pub family: FontFamily<'a>,
    pub size: f32,
    /// 400 / 600 / 700。macOS 映射 kCTFontWeightTrait 0.0 / 0.3 / 0.4。
    pub weight: u16,
    /// 每行在 max_width 内的对齐。
    pub align: TextAlign,
    /// 整段在 max_height 内的对齐。
    pub paragraph: ParagraphAlign,
    /// false = 不换行（字幕替换模式）。
    pub wrap: bool,
    /// None = 字体默认行距（胶囊、悬浮球）。
    pub line_spacing: Option<UniformLineSpacing>,
}

/// 与 DWRITE_TEXT_METRICS 同义：width 不含行尾空白；left/top 相对布局框原点。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextMetrics {
    pub left: f32,
    pub top: f32,
    pub width: f32,
    pub height: f32,
    pub line_count: u32,
}

/// 绘制时把 [utf16_start, utf16_start + utf16_len) 换成另一颜色（新增文字淡入）。两平台字符串索引都是 UTF-16。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextTint {
    pub utf16_start: u32,
    pub utf16_len: u32,
    pub color: Color,
}

/// 排版与测量。与 Canvas 分开：字幕在 apply 阶段（没有画布）就要测量。
/// 实现只在一个线程使用（Windows 窗口线程 / Mac 渲染线程），不要求 Send。
pub trait TextEngine {
    /// 句柄型，Clone 只增引用计数；可缓存在模型里跨帧复用。
    type Layout: Clone;
    /// 布局框原点 (0,0)，尺寸 max_width × max_height。locale 由后端固定为中文简体。
    fn layout(
        &self,
        text: &str,
        style: &TextStyle<'_>,
        max_width: f32,
        max_height: f32,
    ) -> Option<Self::Layout>;
    fn metrics(&self, layout: &Self::Layout) -> TextMetrics;
}

pub type LayoutOf<C> = <<C as Canvas>::Text as TextEngine>::Layout;

/// 一帧的绘图面。坐标为 DIP；帧开始时已清为全透明。
pub trait Canvas {
    type Text: TextEngine;
    fn text(&self) -> &Self::Text;
    /// 96 × 缩放；按设备像素对齐的计算（波形柱）用。
    fn dpi(&self) -> u32;

    /// 保存/恢复变换与裁剪（共用一条栈，严格嵌套）。
    fn save(&mut self);
    fn restore(&mut self);
    /// 硬边裁剪：只保留像素中心落在矩形内的像素（D2D ANTIALIAS_MODE_ALIASED 语义；CG 端取整到设备像素）。
    fn clip_rect(&mut self, rect: Rect);
    fn concat_transform(&mut self, transform: Transform);

    fn fill_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color);
    fn stroke_rounded_rect(&mut self, rect: Rect, radius: f32, color: Color, stroke: Stroke);
    fn fill_circle(&mut self, center: Point, radius: f32, color: Color);
    fn stroke_circle(&mut self, center: Point, radius: f32, color: Color, stroke: Stroke);
    fn line(&mut self, from: Point, to: Point, color: Color, stroke: Stroke);
    /// 填充：每个子路径闭合，nonzero 规则。
    fn fill_icon(&mut self, icon: &SvgIcon, color: Color);
    /// 描边：子路径开放；Z 也只结束子路径（与现有 D2D 构建一致）。
    fn stroke_icon(&mut self, icon: &SvgIcon, color: Color, stroke: Stroke);
    /// 每帧变化、不缓存的描边路径（spinner 弧），语义同 stroke_icon。
    fn stroke_path(&mut self, path: &[SvgSeg], color: Color, stroke: Stroke);

    fn draw_text(
        &mut self,
        layout: &LayoutOf<Self>,
        origin: Point,
        color: Color,
        tint: Option<TextTint>,
    );
    /// 整段换新的模糊入场：布局以 bounds 左上 + offset 绘制，bounds 四周留 18 DIP 后
    /// 用 `blur::fast_blur_premultiplied` 模糊再贴回；参数形状与现有 blurred_text_bitmap 一致，保证浮点顺序。
    fn draw_text_blurred(
        &mut self,
        layout: &LayoutOf<Self>,
        bounds: Rect,
        offset: Point,
        color: Color,
        sigma: f32,
    ) -> Result<(), String>;
}
