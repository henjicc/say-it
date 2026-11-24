//! 开发者「指向测试」：在一段时间里沿着光标轨迹用 UI Automation 取光标下的控件；每认出一次指向，
//! 把动态上下文实际取到的文字（按坐标取行 → 控件名称 → 截图识别）和只用截图识别的结果放在一起，
//! 看各个软件分别走到哪一级、准不准。只在开发者模式下由用户手动启动，不参与听写。
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

use super::dynamic::PointerTracker;
use super::model::ActiveAppContextExtractionMethod;
use super::uia_point::{self, ComScope, ElementInfo, LineAt, PointSource};

const SAMPLE_INTERVAL: Duration = Duration::from_millis(100);
/// 光标在采样之间的细轨迹，喂给停留/绕圈判断，和听写时一样每 33ms 一次。
const TRACK_INTERVAL: Duration = Duration::from_millis(33);
const MAX_ANCESTORS: usize = 8;
/// 认出一次指向时，往前看这么长的轨迹来算「晃动覆盖的最小区域」。
const REGION_WINDOW_MS: u64 = 1_500;
/// 覆盖这一段里至少这么多采样的元素才算这次晃动的区域。
const REGION_COVERAGE: f64 = 0.7;
/// 只用离指向点这么近的采样算区域：从别处移过来的那段路不算。绕圈的外框最大约 240 像素。
const SETTLED_RADIUS: f64 = 130.0;

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
    /// 动态上下文实际会用的文字和它来自哪一级。
    pub(crate) result: Option<String>,
    pub(crate) source: Option<PointSource>,
    pub(crate) result_ms: u64,
    /// 只用截图识别时的结果，用来对比。
    pub(crate) ocr_text: Option<String>,
    pub(crate) ocr_ms: u64,
    /// 按坐标取到的整行和光标在行里的位置，取不到时 `line_error` 写原因。
    pub(crate) line: Option<LineAt>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) line_error: Option<String>,
    /// 光标下的控件（最近一次采样）。
    pub(crate) element: Option<ElementInfo>,
    /// 前 1.5 秒里停下来那段轨迹覆盖的最小区域。
    pub(crate) region: Option<Region>,
    pub(crate) trail_rect: Option<[i32; 4]>,
    pub(crate) samples_used: usize,
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
    let _com = ComScope::enter();
    let automation = uia_point::automation()?;
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
            let (chain, error) = match uia_point::element_chain(&automation, x, y, MAX_ANCESTORS) {
                Ok(chain) => (chain, None),
                Err(error) => (Vec::new(), Some(error)),
            };
            report.samples.push(Sample { t_ms, x, y, uia_ms: sampled.elapsed().as_millis() as u64, chain, error });
        }
        let Some((px, py)) = tracker.push(t_ms, x, y) else { continue };
        let result_started = Instant::now();
        let found = super::dynamic_monitor::pointed_text((px, py), &[], ActiveAppContextExtractionMethod::Ocr, &closed);
        let result_ms = result_started.elapsed().as_millis() as u64;
        let ocr_started = Instant::now();
        let ocr_text = super::dynamic_monitor::window_at(px, py)
            .and_then(|target| super::dynamic_monitor::ocr_pointed_text(target, (px, py), None, &closed));
        let ocr_ms = ocr_started.elapsed().as_millis() as u64;
        let (line, line_error) = match uia_point::line_at(&automation, px, py) {
            Ok(line) => (Some(line), None),
            Err(error) => (None, Some(error)),
        };
        let recent: Vec<&Sample> = report
            .samples
            .iter()
            .filter(|sample| {
                sample.t_ms + REGION_WINDOW_MS >= t_ms
                    && !sample.chain.is_empty()
                    && (((sample.x - px) as f64).powi(2) + ((sample.y - py) as f64).powi(2)).sqrt() <= SETTLED_RADIUS
            })
            .collect();
        report.pointings.push(Pointing {
            t_ms,
            x: px,
            y: py,
            result: found.as_ref().map(|(text, _)| text.clone()),
            source: found.map(|(_, source)| source),
            result_ms,
            ocr_text,
            ocr_ms,
            line,
            line_error,
            element: report.samples.last().and_then(|sample| sample.chain.first().cloned()),
            region: covering_region(&recent),
            trail_rect: trail_rect(&recent),
            samples_used: recent.len(),
        });
    }
    let mut times: Vec<u64> = report.samples.iter().map(|sample| sample.uia_ms).collect();
    times.sort_unstable();
    report.uia_p50_ms = times.get(times.len() / 2).copied().unwrap_or(0);
    report.uia_max_ms = times.last().copied().unwrap_or(0);
    Ok(report)
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
