//! 开发者「指向测试」：在一段时间里沿着光标轨迹用 UI Automation 取光标下的控件，和动态上下文
//! 现有的「停住或绕圈 → 截图 OCR」放在一起记下来，用来判断按控件定位所指区域值不值得做。
//! 只在开发者模式下由用户手动启动，不参与听写。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, TextUnit,
    TextUnit_Line, TextUnit_Paragraph, TextUnit_Word, UIA_TextPatternId,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::dynamic::PointerTracker;
use super::model::ActiveAppContextExtractionMethod;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
/// 光标在采样之间的细轨迹，喂给停留/绕圈判断，和听写时一样每 33ms 一次。
const TRACK_INTERVAL: Duration = Duration::from_millis(33);
const MAX_ANCESTORS: usize = 8;
const NAME_CHARS: usize = 80;
/// 认出一次指向时，往前看这么长的轨迹来算「晃动覆盖的最小区域」。
const REGION_WINDOW_MS: u64 = 1_500;
/// 覆盖这一段里至少这么多采样的元素才算这次晃动的区域。
const REGION_COVERAGE: f64 = 0.7;
/// 只用离指向点这么近的采样算区域：从别处移过来的那段路不算。绕圈的外框最大约 240 像素。
const SETTLED_RADIUS: f64 = 130.0;
/// 往上找能按坐标取文字（文本模式）的元素，最多找这么多层。
const TEXT_PATTERN_DEPTH: usize = 10;
const TEXT_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ElementInfo {
    pub(crate) control_type: String,
    pub(crate) name: String,
    pub(crate) class_name: String,
    /// [left, top, right, bottom]，屏幕物理像素。
    pub(crate) rect: [i32; 4],
}

impl ElementInfo {
    fn area(&self) -> i64 {
        (self.rect[2] - self.rect[0]).max(0) as i64 * (self.rect[3] - self.rect[1]).max(0) as i64
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Sample {
    pub(crate) t_ms: u64,
    pub(crate) x: i32,
    pub(crate) y: i32,
    pub(crate) uia_ms: u64,
    /// 光标下的元素在前，往上是它的祖先；取不到时为空，`error` 写原因。
    pub(crate) chain: Vec<ElementInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Region {
    pub(crate) element: ElementInfo,
    pub(crate) coverage: f64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Pointing {
    pub(crate) t_ms: u64,
    pub(crate) x: i32,
    pub(crate) y: i32,
    /// 现有做法：截图 OCR 取光标所在那一小段。
    pub(crate) ocr_text: Option<String>,
    pub(crate) ocr_ms: u64,
    /// 光标下的控件（最近一次采样）。
    pub(crate) element: Option<ElementInfo>,
    /// 前 1.5 秒轨迹覆盖的最小区域。
    pub(crate) region: Option<Region>,
    /// 前 1.5 秒里停下来那段轨迹的外框。
    pub(crate) trail_rect: Option<[i32; 4]>,
    pub(crate) samples_used: usize,
    /// 按坐标从文本模式取到的词、行、段，取不到时为空，`text_error` 写原因。
    pub(crate) text_at: Option<TextAt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) text_error: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TextAt {
    pub(crate) word: String,
    pub(crate) line: String,
    pub(crate) paragraph: String,
    /// 提供文本模式的元素。
    pub(crate) source: ElementInfo,
    pub(crate) ms: u64,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ProbeReport {
    pub(crate) samples: Vec<Sample>,
    pub(crate) pointings: Vec<Pointing>,
    pub(crate) uia_p50_ms: u64,
    pub(crate) uia_max_ms: u64,
}

/// 采样 `duration`，`cancelled` 置位时提前结束。在调用线程上跑，调用方给它单独开线程。
pub(crate) fn run(duration: Duration, cancelled: &AtomicBool) -> Result<ProbeReport, String> {
    let initialized = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() };
    let result = (|| {
        let automation: IUIAutomation = unsafe { CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER) }
            .map_err(|error| format!("无法初始化 UI Automation：{error}"))?;
        Ok(sample_for(&automation, duration, cancelled))
    })();
    if initialized {
        unsafe { CoUninitialize() };
    }
    result
}

fn sample_for(automation: &IUIAutomation, duration: Duration, cancelled: &AtomicBool) -> ProbeReport {
    let started = Instant::now();
    let mut tracker = PointerTracker::new(cursor().unwrap_or_default());
    let mut report = ProbeReport::default();
    let mut next_sample = started;
    let closed = Arc::new(AtomicBool::new(false));
    while started.elapsed() < duration && !cancelled.load(Ordering::Acquire) {
        std::thread::sleep(TRACK_INTERVAL);
        let Some((x, y)) = cursor() else { continue };
        let t_ms = started.elapsed().as_millis() as u64;
        if Instant::now() >= next_sample {
            next_sample += SAMPLE_INTERVAL;
            let sampled = Instant::now();
            let (chain, error) = match element_chain(automation, x, y) {
                Ok(chain) => (chain, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            report.samples.push(Sample { t_ms, x, y, uia_ms: sampled.elapsed().as_millis() as u64, chain, error });
        }
        if let Some((px, py)) = tracker.push(t_ms, x, y) {
            let ocr_started = Instant::now();
            let ocr_text = super::dynamic_monitor::pointed_text(
                (px, py),
                &[],
                ActiveAppContextExtractionMethod::Ocr,
                &closed,
            );
            let recent: Vec<&Sample> = report
                .samples
                .iter()
                .filter(|sample| {
                    sample.t_ms + REGION_WINDOW_MS >= t_ms
                        && !sample.chain.is_empty()
                        && (((sample.x - px) as f64).powi(2) + ((sample.y - py) as f64).powi(2)).sqrt() <= SETTLED_RADIUS
                })
                .collect();
            let (text_at, text_error) = match text_at(automation, px, py) {
                Ok(text) => (Some(text), None),
                Err(error) => (None, Some(error)),
            };
            report.pointings.push(Pointing {
                t_ms,
                x: px,
                y: py,
                ocr_text,
                ocr_ms: ocr_started.elapsed().as_millis() as u64,
                element: report.samples.last().and_then(|sample| sample.chain.first().cloned()),
                region: covering_region(&recent),
                trail_rect: trail_rect(&recent),
                samples_used: recent.len(),
                text_at,
                text_error,
            });
        }
    }
    let mut times: Vec<u64> = report.samples.iter().map(|sample| sample.uia_ms).collect();
    times.sort_unstable();
    report.uia_p50_ms = times.get(times.len() / 2).copied().unwrap_or(0);
    report.uia_max_ms = times.last().copied().unwrap_or(0);
    report
}

/// 光标下的元素和它的祖先（不含桌面根）。光标在本应用自己的窗口上时不取。
fn element_chain(automation: &IUIAutomation, x: i32, y: i32) -> Result<Vec<ElementInfo>, String> {
    unsafe {
        let element = automation
            .ElementFromPoint(POINT { x, y })
            .map_err(|error| format!("ElementFromPoint 失败：{error}"))?;
        if element.CurrentProcessId().ok() == Some(std::process::id() as i32) {
            return Err("光标在说吧自己的窗口上".into());
        }
        let walker = automation.ControlViewWalker().map_err(|error| format!("取控件树失败：{error}"))?;
        let root = automation.GetRootElement().map_err(|error| format!("取桌面根失败：{error}"))?;
        let mut chain = vec![info(&element)?];
        let mut current = element;
        while chain.len() <= MAX_ANCESTORS {
            let Ok(parent) = walker.GetParentElement(&current) else { break };
            if automation.CompareElements(&parent, &root).is_ok_and(|same| same.as_bool()) {
                break;
            }
            chain.push(info(&parent)?);
            current = parent;
        }
        Ok(chain)
    }
}

/// 光标下那个字所在的词、行和段：从光标下的元素往上找第一个支持文本模式的元素，按坐标取文字范围。
fn text_at(automation: &IUIAutomation, x: i32, y: i32) -> Result<TextAt, String> {
    let started = Instant::now();
    unsafe {
        let walker = automation.ControlViewWalker().map_err(|error| format!("取控件树失败：{error}"))?;
        let mut element = automation
            .ElementFromPoint(POINT { x, y })
            .map_err(|error| format!("ElementFromPoint 失败：{error}"))?;
        for _ in 0..TEXT_PATTERN_DEPTH {
            if let Ok(pattern) = element.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                let range = pattern
                    .RangeFromPoint(POINT { x, y })
                    .map_err(|error| format!("RangeFromPoint 失败：{error}"))?;
                let unit = |unit: TextUnit| -> String {
                    range
                        .Clone()
                        .and_then(|copy| {
                            copy.ExpandToEnclosingUnit(unit)?;
                            copy.GetText(TEXT_CHARS as i32)
                        })
                        .map(|text| text.to_string().trim().to_string())
                        .unwrap_or_default()
                };
                return Ok(TextAt {
                    word: unit(TextUnit_Word),
                    line: unit(TextUnit_Line),
                    paragraph: unit(TextUnit_Paragraph),
                    source: info(&element)?,
                    ms: started.elapsed().as_millis() as u64,
                });
            }
            element = walker
                .GetParentElement(&element)
                .map_err(|_| "往上没有支持文本模式的元素".to_string())?;
        }
        Err(format!("往上 {TEXT_PATTERN_DEPTH} 层都没有支持文本模式的元素"))
    }
}

fn info(element: &IUIAutomationElement) -> Result<ElementInfo, String> {
    unsafe {
        let rect = element.CurrentBoundingRectangle().map_err(|error| format!("读控件边框失败：{error}"))?;
        let name = element.CurrentName().map(|value| value.to_string()).unwrap_or_default();
        Ok(ElementInfo {
            control_type: element.CurrentLocalizedControlType().map(|value| value.to_string()).unwrap_or_default(),
            name: name.chars().take(NAME_CHARS).collect(),
            class_name: element.CurrentClassName().map(|value| value.to_string().chars().take(NAME_CHARS).collect()).unwrap_or_default(),
            rect: [rect.left, rect.top, rect.right, rect.bottom],
        })
    }
}

/// 晃动覆盖的最小区域：在这段采样里出现得够多（至少七成采样的祖先链里有它）的元素中，取面积最小的。
fn covering_region(samples: &[&Sample]) -> Option<Region> {
    if samples.is_empty() {
        return None;
    }
    let mut candidates: Vec<(ElementInfo, usize)> = Vec::new();
    for sample in samples {
        for element in &sample.chain {
            match candidates.iter_mut().find(|(known, _)| known == element) {
                Some((_, count)) => *count += 1,
                None => candidates.push((element.clone(), 1)),
            }
        }
    }
    candidates
        .into_iter()
        .map(|(element, count)| (element, count as f64 / samples.len() as f64))
        .filter(|(element, coverage)| *coverage >= REGION_COVERAGE && element.area() > 0)
        .min_by_key(|(element, _)| element.area())
        .map(|(element, coverage)| Region { element, coverage })
}

fn trail_rect(samples: &[&Sample]) -> Option<[i32; 4]> {
    let first = samples.first()?;
    Some(samples.iter().fold([first.x, first.y, first.x, first.y], |rect, sample| {
        [rect[0].min(sample.x), rect[1].min(sample.y), rect[2].max(sample.x), rect[3].max(sample.y)]
    }))
}

fn cursor() -> Option<(i32, i32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.is_ok().then_some((point.x, point.y))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn element(name: &str, rect: [i32; 4]) -> ElementInfo {
        ElementInfo { control_type: "按钮".into(), name: name.into(), class_name: String::new(), rect }
    }

    fn sample(chain: Vec<ElementInfo>) -> Sample {
        Sample { t_ms: 0, x: 0, y: 0, uia_ms: 0, chain, error: None }
    }

    #[test]
    fn region_is_the_smallest_element_most_of_the_wiggle_stays_in() {
        let list = element("列表", [0, 0, 400, 400]);
        let first = element("第一项", [0, 0, 400, 40]);
        let second = element("第二项", [0, 40, 400, 80]);
        // 一直在一项上晃：取这一项。
        let still = [sample(vec![first.clone(), list.clone()]), sample(vec![first.clone(), list.clone()])];
        assert_eq!(covering_region(&still.iter().collect::<Vec<_>>()).map(|region| region.element), Some(first.clone()));
        // 在两项之间来回晃：取它们所在的列表。
        let across = [
            sample(vec![first.clone(), list.clone()]),
            sample(vec![second.clone(), list.clone()]),
            sample(vec![first, list.clone()]),
            sample(vec![second, list.clone()]),
        ];
        assert_eq!(covering_region(&across.iter().collect::<Vec<_>>()).map(|region| region.element), Some(list));
    }
}
