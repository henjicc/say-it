/// 极简 SVG path 段：只支持悬浮窗图标用到的命令集（M/L/H/V/C/S/A/Z）。
/// 解析后换算为绝对坐标，供平台后端构建路径。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SvgSeg {
    Move(f32, f32),
    Line(f32, f32),
    Bezier(f32, f32, f32, f32, f32, f32),
    Arc {
        x: f32,
        y: f32,
        rx: f32,
        ry: f32,
        rotation: f32,
        large: bool,
        sweep: bool,
    },
    Close,
}

/// 解析 SVG path 的 d 属性。命令与数值可以无分隔连写（如 `1.66-3`、`a.84.84 0 0 0-.83-.71`）。
pub fn parse_svg_path(d: &str) -> Result<Vec<SvgSeg>, String> {
    let bytes = d.as_bytes();
    let mut i = 0usize;

    fn skip_seps(bytes: &[u8], i: &mut usize) {
        while *i < bytes.len() && (bytes[*i].is_ascii_whitespace() || bytes[*i] == b',') {
            *i += 1;
        }
    }

    fn scan_number(bytes: &[u8], i: &mut usize) -> Result<f32, String> {
        skip_seps(bytes, i);
        let start = *i;
        let mut seen_dot = false;
        let mut seen_exp = false;
        while *i < bytes.len() {
            let c = bytes[*i];
            match c {
                b'0'..=b'9' => *i += 1,
                b'.' if !seen_dot && !seen_exp => {
                    seen_dot = true;
                    *i += 1;
                }
                b'-' | b'+' => {
                    if *i == start {
                        // 数字起始的符号属于当前数字。
                        *i += 1;
                    } else if (bytes[*i - 1] | 0x20) == b'e' {
                        // 指数部分的符号。
                        *i += 1;
                    } else {
                        // 其他位置的符号是下一个 token 的起始。
                        break;
                    }
                }
                b'e' | b'E' if !seen_exp => {
                    seen_exp = true;
                    *i += 1;
                }
                _ => break,
            }
        }
        if *i == start {
            return Err(format!("SVG path 在位置 {start} 缺少数值"));
        }
        std::str::from_utf8(&bytes[start..*i])
            .ok()
            .and_then(|text| text.parse::<f32>().ok())
            .ok_or_else(|| format!("SVG path 数值解析失败（位置 {start}）"))
    }

    let mut segments = Vec::new();
    let mut command: Option<u8> = None;
    let mut current = (0.0f32, 0.0f32);
    let mut figure_start = (0.0f32, 0.0f32);
    let mut last_control: Option<(f32, f32)> = None;

    while i < bytes.len() {
        skip_seps(bytes, &mut i);
        if i >= bytes.len() {
            break;
        }
        let c = bytes[i];
        if c.is_ascii_alphabetic() {
            if c == b'z' || c == b'Z' {
                segments.push(SvgSeg::Close);
                current = figure_start;
                last_control = None;
                command = None;
                i += 1;
                continue;
            }
            command = Some(c);
            i += 1;
        }
        let Some(c) = command else {
            return Err("SVG path 以数值开头，缺少命令字母".to_string());
        };
        let relative = c.is_ascii_lowercase();
        let upper = c.to_ascii_uppercase();
        match upper {
            b'M' => {
                let x = scan_number(bytes, &mut i)?;
                let y = scan_number(bytes, &mut i)?;
                let (x, y) = if relative {
                    (current.0 + x, current.1 + y)
                } else {
                    (x, y)
                };
                segments.push(SvgSeg::Move(x, y));
                current = (x, y);
                figure_start = current;
                last_control = None;
                // 后续坐标对按隐式 LineTo 处理。
                command = Some(if relative { b'l' } else { b'L' });
            }
            b'L' => {
                let x = scan_number(bytes, &mut i)?;
                let y = scan_number(bytes, &mut i)?;
                let (x, y) = if relative {
                    (current.0 + x, current.1 + y)
                } else {
                    (x, y)
                };
                segments.push(SvgSeg::Line(x, y));
                current = (x, y);
                last_control = None;
            }
            b'H' => {
                let x = scan_number(bytes, &mut i)?;
                let x = if relative { current.0 + x } else { x };
                segments.push(SvgSeg::Line(x, current.1));
                current.0 = x;
                last_control = None;
            }
            b'V' => {
                let y = scan_number(bytes, &mut i)?;
                let y = if relative { current.1 + y } else { y };
                segments.push(SvgSeg::Line(current.0, y));
                current.1 = y;
                last_control = None;
            }
            b'C' => {
                let c1x = scan_number(bytes, &mut i)?;
                let c1y = scan_number(bytes, &mut i)?;
                let c2x = scan_number(bytes, &mut i)?;
                let c2y = scan_number(bytes, &mut i)?;
                let x = scan_number(bytes, &mut i)?;
                let y = scan_number(bytes, &mut i)?;
                let (c1x, c1y) = if relative {
                    (current.0 + c1x, current.1 + c1y)
                } else {
                    (c1x, c1y)
                };
                let (c2x, c2y) = if relative {
                    (current.0 + c2x, current.1 + c2y)
                } else {
                    (c2x, c2y)
                };
                let (x, y) = if relative {
                    (current.0 + x, current.1 + y)
                } else {
                    (x, y)
                };
                segments.push(SvgSeg::Bezier(c1x, c1y, c2x, c2y, x, y));
                last_control = Some((c2x, c2y));
                current = (x, y);
            }
            b'S' => {
                // 平滑三次贝塞尔：第一控制点取上一段第二控制点关于当前点的镜像。
                let c1 = last_control
                    .map(|p| (2.0 * current.0 - p.0, 2.0 * current.1 - p.1))
                    .unwrap_or(current);
                let c2x = scan_number(bytes, &mut i)?;
                let c2y = scan_number(bytes, &mut i)?;
                let x = scan_number(bytes, &mut i)?;
                let y = scan_number(bytes, &mut i)?;
                let (c2x, c2y) = if relative {
                    (current.0 + c2x, current.1 + c2y)
                } else {
                    (c2x, c2y)
                };
                let (x, y) = if relative {
                    (current.0 + x, current.1 + y)
                } else {
                    (x, y)
                };
                segments.push(SvgSeg::Bezier(c1.0, c1.1, c2x, c2y, x, y));
                last_control = Some((c2x, c2y));
                current = (x, y);
            }
            b'A' => {
                let rx = scan_number(bytes, &mut i)?;
                let ry = scan_number(bytes, &mut i)?;
                let rotation = scan_number(bytes, &mut i)?;
                let large = scan_number(bytes, &mut i)? != 0.0;
                let sweep = scan_number(bytes, &mut i)? != 0.0;
                let x = scan_number(bytes, &mut i)?;
                let y = scan_number(bytes, &mut i)?;
                let (x, y) = if relative {
                    (current.0 + x, current.1 + y)
                } else {
                    (x, y)
                };
                segments.push(SvgSeg::Arc {
                    x,
                    y,
                    rx,
                    ry,
                    rotation,
                    large,
                    sweep,
                });
                current = (x, y);
                last_control = None;
            }
            other => return Err(format!("SVG path 命令 {other} 暂不支持")),
        }
    }
    Ok(segments)
}

/// SVG 端点式椭圆弧转为每段不超过 90° 的三次贝塞尔（W3C SVG B.2）。
/// 零半径返回直线，重合端点返回空；不足的半径按 Λ 放大。
pub fn arc_to_cubics(from: (f32, f32), arc: &SvgSeg) -> Vec<[(f32, f32); 3]> {
    let SvgSeg::Arc {
        x,
        y,
        rx,
        ry,
        rotation,
        large,
        sweep,
    } = *arc
    else {
        return Vec::new();
    };
    if ![from.0, from.1, x, y, rx, ry, rotation]
        .iter()
        .all(|v| v.is_finite())
        || from == (x, y)
    {
        return Vec::new();
    }
    if rx == 0.0 || ry == 0.0 {
        return vec![[from, (x, y), (x, y)]];
    }
    // 中间计算用 f64，避免半径修正与中心求解的消减误差。
    let (mut rx, mut ry) = (f64::from(rx).abs(), f64::from(ry).abs());
    let (sin, cos) = f64::from(rotation).to_radians().sin_cos();
    let (dx, dy) = (
        (f64::from(from.0) - f64::from(x)) / 2.0,
        (f64::from(from.1) - f64::from(y)) / 2.0,
    );
    let (xp, yp) = (cos * dx + sin * dy, -sin * dx + cos * dy);
    let lambda = (xp / rx).powi(2) + (yp / ry).powi(2);
    if lambda > 1.0 {
        rx *= lambda.sqrt();
        ry *= lambda.sqrt();
    }
    let denominator = rx * rx * yp * yp + ry * ry * xp * xp;
    let factor = ((rx * rx * ry * ry - denominator).max(0.0) / denominator).sqrt()
        * if large == sweep { -1.0 } else { 1.0 };
    let (cxp, cyp) = (factor * rx * yp / ry, -factor * ry * xp / rx);
    let (cx, cy) = (
        cos * cxp - sin * cyp + (f64::from(from.0) + f64::from(x)) / 2.0,
        sin * cxp + cos * cyp + (f64::from(from.1) + f64::from(y)) / 2.0,
    );
    let (ux, uy) = ((xp - cxp) / rx, (yp - cyp) / ry);
    let (vx, vy) = ((-xp - cxp) / rx, (-yp - cyp) / ry);
    let start = uy.atan2(ux);
    let mut delta = (ux * vy - uy * vx).atan2(ux * vx + uy * vy);
    if sweep && delta < 0.0 {
        delta += std::f64::consts::TAU;
    }
    if !sweep && delta > 0.0 {
        delta -= std::f64::consts::TAU;
    }
    let count = (delta.abs() / std::f64::consts::FRAC_PI_2).ceil() as usize;
    let step = delta / count as f64;
    let map = |u, v| {
        (
            (cx + rx * cos * u - ry * sin * v) as f32,
            (cy + rx * sin * u + ry * cos * v) as f32,
        )
    };
    let mut result = Vec::with_capacity(count);
    for i in 0..count {
        let a = start + i as f64 * step;
        let b = a + step;
        let k = 4.0 / 3.0 * (step / 4.0).tan();
        result.push([
            map(a.cos() - k * a.sin(), a.sin() + k * a.cos()),
            map(b.cos() + k * b.sin(), b.sin() - k * b.cos()),
            map(b.cos(), b.sin()),
        ]);
    }
    if let Some(last) = result.last_mut() {
        last[2] = (x, y);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arc(end: (f32, f32), radius: (f32, f32), large: bool, sweep: bool) -> SvgSeg {
        SvgSeg::Arc {
            x: end.0,
            y: end.1,
            rx: radius.0,
            ry: radius.1,
            rotation: 0.0,
            large,
            sweep,
        }
    }

    #[test]
    fn arc_flags_select_short_long_and_direction() {
        for large in [false, true] {
            for sweep in [false, true] {
                let curves =
                    arc_to_cubics((10.0, 0.0), &arc((0.0, 10.0), (10.0, 10.0), large, sweep));
                assert_eq!(curves.len(), if large { 3 } else { 1 });
                assert_eq!(curves.last().unwrap()[2], (0.0, 10.0));
                let control = curves[0][0];
                match (large, sweep) {
                    (false, true) => assert!(control.1 > 0.0),
                    (false, false) => assert!(control.0 < 10.0),
                    (true, true) => assert!(control.0 > 10.0),
                    (true, false) => assert!(control.1 < 0.0),
                }
            }
        }
    }

    #[test]
    fn arc_handles_degenerate_and_corrected_radii() {
        assert!(arc_to_cubics((0.0, 0.0), &arc((0.0, 0.0), (2.0, 3.0), false, true)).is_empty());
        assert_eq!(
            arc_to_cubics((0.0, 0.0), &arc((8.0, 0.0), (0.0, 3.0), false, true)),
            vec![[(0.0, 0.0), (8.0, 0.0), (8.0, 0.0)]]
        );
        let curves = arc_to_cubics((0.0, 0.0), &arc((10.0, 0.0), (1.0, 1.0), false, true));
        assert_eq!(curves.len(), 2);
        assert!((curves[0][2].0 - 5.0).abs() < 1e-5);
        assert!((curves[0][2].1 + 5.0).abs() < 1e-5);
        assert_eq!(
            curves,
            arc_to_cubics((0.0, 0.0), &arc((10.0, 0.0), (-1.0, -1.0), false, true))
        );
    }

    #[test]
    fn arc_spinner_100_degrees_and_rotated_ellipse() {
        let angle = 100.0f32.to_radians();
        let end = (8.0 * angle.cos(), 8.0 * angle.sin());
        let curves = arc_to_cubics((8.0, 0.0), &arc(end, (8.0, 8.0), false, true));
        assert_eq!(curves.len(), 2);
        assert!((curves[0][2].0 - 8.0 * (angle / 2.0).cos()).abs() < 1e-5);
        assert_eq!(curves.last().unwrap()[2], end);
        let rotated = SvgSeg::Arc {
            x: 0.0,
            y: 10.0,
            rx: 10.0,
            ry: 5.0,
            rotation: 90.0,
            large: false,
            sweep: true,
        };
        let curves = arc_to_cubics((5.0, 0.0), &rotated);
        assert!(curves.len() <= 2);
        assert!(curves[0][0].1 > 0.0);
        assert_eq!(curves.last().unwrap()[2], (0.0, 10.0));
    }
    const MIC: &str = "M12 15c1.66 0 2.99-1.34 2.99-3L15 6c0-1.66-1.34-3-3-3S9 4.34 9 6v6c0 1.66 1.34 3 3 3m6.08-3c-.42 0-.77.3-.83.71c-.37 2.61-2.72 4.39-5.25 4.39s-4.88-1.77-5.25-4.39a.84.84 0 0 0-.83-.71c-.52 0-.92.46-.85.97c.46 2.97 2.96 5.3 5.93 5.75V21c0 .55.45 1 1 1s1-.45 1-1v-2.28c2.96-.43 5.47-2.78 5.93-5.75a.857.857 0 0 0-.85-.97";

    #[test]
    fn svg_path_parses_mic_icon() {
        let segs = parse_svg_path(MIC).unwrap();
        assert_eq!(segs[0], SvgSeg::Move(12.0, 15.0));
        // 相对命令换算为绝对坐标。
        assert!(segs.iter().any(|seg| matches!(seg, SvgSeg::Move(x, y) if (*x - 18.08).abs() < 1e-4 && (*y - 12.0).abs() < 1e-4)));
        // 两段圆弧：SVG 相对终点换算正确。
        let arcs: Vec<_> = segs
            .iter()
            .filter(|seg| matches!(seg, SvgSeg::Arc { .. }))
            .collect();
        assert_eq!(arcs.len(), 2);
        if let SvgSeg::Arc {
            x,
            y,
            rx,
            large,
            sweep,
            ..
        } = arcs[0]
        {
            assert!((*x - 5.92).abs() < 1e-3 && (*y - 12.0).abs() < 1e-3);
            assert!((*rx - 0.84).abs() < 1e-4);
            assert!(!large && !sweep);
        } else {
            panic!("第一段应为圆弧");
        }
        // 平滑贝塞尔 S 的镜像控制点已展开为普通 Bezier。
        assert!(segs.iter().all(|seg| !matches!(seg, SvgSeg::Close)));
    }

    #[test]
    fn svg_path_parses_packed_numbers() {
        // 无分隔连写：负号与小数点作为新 token 起始。
        let segs = parse_svg_path("M0 0c1.66 0 2.99-1.34 2.99-3").unwrap();
        assert_eq!(segs[1], SvgSeg::Bezier(1.66, 0.0, 2.99, -1.34, 2.99, -3.0));
    }
}
