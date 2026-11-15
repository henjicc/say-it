//! 悬浮球设置面板的原生实现：窗口声明、对外命令与动作派发。
//!
//! 模型在 overlay-core 的 `orb_menu`；设置都存在 `floating_orb`，这里只把快照投给面板，
//! 把面板里的操作转成那边的设置命令。

use super::native_common::{post, post_if_started, run_action};
use super::native_test::{self, TestWindow};
use sayit_overlay_core::canvas::TextEngine;
use sayit_overlay_core::geom::PxRect;
use sayit_overlay_core::look::Look;
use sayit_overlay_core::orb_menu::{Command, OrbMenu, OrbMenuAction, OrbMenuView};
use sayit_overlay_core::spec::{OverlaySpec, Placement};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::AppHandle;

static APP: OnceLock<AppHandle> = OnceLock::new();
static VISIBLE: AtomicBool = AtomicBool::new(false);
/// 拖滑块时最新的大小与不透明度，0 表示没有待写入的值（取值范围不含 0）。
/// 拖动会连续发出很多次，后台按最新值写，不按到达顺序逐个写。
static PENDING_SIZE: AtomicU16 = AtomicU16::new(0);
static PENDING_OPACITY: AtomicU8 = AtomicU8::new(0);
static APPLYING: Mutex<()> = Mutex::new(());

pub(crate) struct OrbMenuWindow;
impl OverlaySpec for OrbMenuWindow {
    const NAME: &'static str = "sayit-native-orb-menu";
    const TITLE: &'static str = "说吧！悬浮球设置";
    const PLACEMENT: Placement = Placement::Free;
    const CLICK_THROUGH: bool = false;
    const FOCUSABLE: bool = true;
    type Command = Command;
    type Action = OrbMenuAction;
    type Model<T: TextEngine + 'static> = OrbMenu;
    fn model<T: TextEngine + 'static>(dpi: u32, look: &Look) -> OrbMenu {
        OrbMenu::new(dpi, look)
    }
    fn dispatch(action: OrbMenuAction) {
        if native_test::testing(TestWindow::OrbMenu) {
            // 测试中只响应收起，其余操作只有面板自己的视觉反馈。
            if matches!(action, OrbMenuAction::Close | OrbMenuAction::Focused(false)) {
                native_test::closed_by_button(TestWindow::OrbMenu);
            }
            return;
        }
        run_action(move || run(action));
    }
}

pub(crate) fn native_orb_menu_show(app: &AppHandle, view: OrbMenuView, orb: PxRect, work: PxRect) {
    let _ = APP.set(app.clone());
    super::native_common::native_overlay_start();
    super::native_common::native_overlay_set_look(super::floating_orb::native_look(app));
    VISIBLE.store(true, Ordering::Release);
    post::<OrbMenuWindow>(Command::Show {
        view,
        orb,
        work,
        focus: true,
    });
}

/// 设置变了（面板、主窗口或其它入口改的）就刷新面板；面板没开时什么也不做。
pub(crate) fn native_orb_menu_update(view: OrbMenuView) {
    if VISIBLE.load(Ordering::Acquire) {
        post_if_started::<OrbMenuWindow>(Command::Update(view));
    }
}

pub(crate) fn native_orb_menu_hide() {
    if VISIBLE.swap(false, Ordering::AcqRel) {
        post_if_started::<OrbMenuWindow>(Command::Hide);
    }
}

fn show_error(app: &AppHandle, error: String) {
    let settings = super::floating_orb::floating_orb_settings(app);
    native_orb_menu_update(super::floating_orb::orb_menu_view(&settings, Some(error)));
}

/// 按最新的待写值改大小与不透明度；同一时间只有一个在写。
fn apply_display(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let Ok(_guard) = APPLYING.lock() else {
            return;
        };
        let size = PENDING_SIZE.swap(0, Ordering::AcqRel);
        let opacity = PENDING_OPACITY.swap(0, Ordering::AcqRel);
        if size == 0 && opacity == 0 {
            return;
        }
        let settings = super::floating_orb::floating_orb_settings(&app);
        let result = super::floating_orb::set_floating_orb_appearance(
            app.clone(),
            if size == 0 { settings.size_percent } else { size },
            if opacity == 0 { settings.opacity } else { opacity },
            settings.glass_enabled,
            settings.glass_material,
            settings.glass_tint,
            settings.glass_border,
        );
        if let Err(error) = result {
            show_error(&app, error);
        }
    });
}

fn run(action: OrbMenuAction) {
    let Some(app) = APP.get().cloned() else {
        return;
    };
    use super::floating_orb as orb;
    match action {
        OrbMenuAction::SetSize(size) => {
            PENDING_SIZE.store(size, Ordering::Release);
            apply_display(app);
        }
        OrbMenuAction::SetOpacity(opacity) => {
            PENDING_OPACITY.store(opacity, Ordering::Release);
            apply_display(app);
        }
        OrbMenuAction::ResetDisplay => {
            PENDING_SIZE.store(crate::state::DEFAULT_FLOATING_ORB_SIZE_PERCENT, Ordering::Release);
            PENDING_OPACITY.store(crate::state::DEFAULT_FLOATING_ORB_OPACITY, Ordering::Release);
            apply_display(app);
        }
        OrbMenuAction::SetAutoEnter(enabled) => {
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = orb::set_floating_orb_auto_enter(app.clone(), enabled) {
                    show_error(&app, error);
                }
            });
        }
        OrbMenuAction::OpenMainWindow => {
            tauri::async_runtime::spawn(async move {
                if let Err(error) = orb::floating_orb_open_main_window(app.clone()).await {
                    show_error(&app, error);
                }
            });
        }
        OrbMenuAction::DisableOrb => {
            tauri::async_runtime::spawn_blocking(move || {
                if let Err(error) = orb::set_floating_orb_enabled(app.clone(), false) {
                    show_error(&app, error);
                }
            });
        }
        OrbMenuAction::Close | OrbMenuAction::Focused(false) => native_orb_menu_hide(),
        OrbMenuAction::Focused(true) => {}
    }
}
