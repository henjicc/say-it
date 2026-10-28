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

#[cfg(test)]
mod tests {
    use super::*;
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
