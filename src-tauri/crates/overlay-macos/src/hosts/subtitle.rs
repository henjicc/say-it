//! 实时字幕条宿主入口。
use crate::ct::CtText;
use crate::render::{self, Host, WindowId};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::screen;
use sayit_overlay_core::subtitle::{Command, Subtitle};
use std::sync::Once;

type Model = Subtitle<<CtText as TextEngine>::Layout>;

fn register() {
    static REGISTER: Once = Once::new();
    REGISTER.call_once(|| {
        render::install_host(
            WindowId::Subtitle,
            Box::new(|cx| {
                let dpi = cx
                    .screens
                    .area_screen()
                    .map_or(96, |s| screen::dpi_for_scale(s.scale));
                let host = Host::new(
                    WindowId::Subtitle,
                    Model::new(dpi, &cx.look),
                    dpi,
                    crate::OverlayEvent::Subtitle,
                );
                host.attach();
                Box::new(host)
            }),
        );
    });
}

/// 可在任意线程调用；模型与 CoreText 只在渲染线程创建及使用。
pub fn post(command: Command) {
    register();
    render::run_on_host(
        WindowId::Subtitle,
        Box::new(move |host, cx| {
            host.as_any_mut()
                .downcast_mut::<Host<Model>>()
                .expect("subtitle host type")
                .apply(command, cx);
        }),
    );
}

/// 字幕百分比布局的逻辑参考尺寸，与模型布局区同屏；屏幕快照尚未采集时为 None。
pub fn reference_size() -> Option<(f64, f64)> {
    let snapshot = crate::screens::snapshot();
    snapshot
        .area_screen()
        .map(|s| (s.visible.width, s.visible.height))
}
