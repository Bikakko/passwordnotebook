//! 主窗口:唯一的顶层窗口,承载四种形态。
//!
//! 退出路径刻意做成确定性的:窗口被销毁 → `WM_DESTROY` → `PostQuitMessage`
//! → 消息循环退出 → `run()` 返回 → 进程结束。没有任何后台线程持有进程,
//! 所以关闭窗口后进程会立即消失。

use std::ffi::c_void;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{DefWindowProcW, MINMAXINFO, PostQuitMessage};

use super::app::{self, Mode};
use super::sys::*;
use super::{dialog, main_ui, ui};

pub const CLASS_NAME: &str = "PasswordNotebookMainWindow";

const TIMER_AUTOCLOSE: usize = 1;

pub fn run_main() -> i32 {
    run_main_inner(None)
}

/// `auto_close_ms` 仅用于自检:窗口显示后自动关闭,用来验证退出路径。
pub fn run_main_inner(auto_close_ms: Option<u32>) -> i32 {
    // 类可能已经注册过(自检会跑两次窗口),注册失败不必中断。
    let _ = ui::register_class(CLASS_NAME, wnd_proc);

    setup_state();

    // 状态随窗口生命周期存在:窗口销毁后由本函数回收。
    let main_ui = Box::new(main_ui::MainUi::new());
    let param = &*main_ui as *const main_ui::MainUi as *const c_void;

    // 用系统 DPI 初始化:`WM_CREATE` 阶段窗口还没关联显示器,拿不到正确的 DPI。
    app::state().dpi = ui::system_dpi();

    // 注意:x 传 CW_USEDEFAULT 时,系统会**忽略**宽高参数而使用默认小尺寸,
    // 所以这里自己算 DPI 缩放后的尺寸并居中 —— 否则窗口一开就又小又挤。
    let (w, h, x, y) = initial_geometry(app::state().dpi);

    let window_title = super::app_title();
    let hwnd = ui::create_window_with_param(
        CLASS_NAME,
        &window_title,
        OVERLAPPED_WINDOW | WS_CLIPCHILDREN,
        WS_EX_CONTROLPARENT | WS_EX_APPWINDOW,
        HWND::default(),
        0,
        x,
        y,
        w,
        h,
        param,
    );

    if hwnd.is_invalid() {
        return 1;
    }

    app::state().main = hwnd;

    // 订阅会话状态变化,用于在锁屏(Win+L)时立即锁定密码本。
    unsafe {
        let _ = windows::Win32::System::RemoteDesktop::WTSRegisterSessionNotification(
            hwnd,
            NOTIFY_FOR_THIS_SESSION,
        );
    }

    // 还原上次的窗口位置/尺寸/最大化状态(存在注册表,解锁前就能读到)。
    let show_cmd = match super::window_state::load() {
        Some(placement) => {
            let _ = super::window_state::restore(hwnd, &placement);

            // SetWindowPlacement 会绕过最小尺寸限制,可能把窗口还原得比允许的还小,
            // 那样布局会被挤坏。太小就回到默认几何。
            let (_, _, restored_w, restored_h) = ui::window_rect(hwnd);
            let (min_w, min_h) = minimum_size(app::state().dpi);
            if restored_w < min_w || restored_h < min_h {
                let (w, h, x, y) = initial_geometry(app::state().dpi);
                unsafe {
                    let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowPos(
                        hwnd,
                        None,
                        x,
                        y,
                        w,
                        h,
                        windows::Win32::UI::WindowsAndMessaging::SWP_NOZORDER,
                    );
                }
            }

            // 上次是最小化的话不要还原成最小化,否则一打开就是收起来的。
            if placement.showCmd == SW_SHOWMINIMIZED as u32 {
                SW_SHOW
            } else {
                placement.showCmd as i32
            }
        }
        None => SW_SHOW,
    };

    ui::show_window(hwnd, show_cmd);
    ui::update_window(hwnd);

    if let Some(ms) = auto_close_ms {
        ui::set_timer(hwnd, TIMER_AUTOCLOSE, ms);
    } else {
        // 正常启动时尝试静默免密解锁。
        main_ui::try_auto_unlock(hwnd);
    }

    dialog::run_modal(hwnd);
    drop(main_ui);
    0
}

/// 允许的最小窗口尺寸(与 WM_GETMINMAXINFO 保持一致)。
fn minimum_size(dpi: u32) -> (i32, i32) {
    let dpi = dpi.max(96);
    let scale = |v: i32| (v as f32 * dpi as f32 / 96.0).round() as i32;
    (scale(900), scale(600))
}

/// 按系统 DPI 缩放后在主屏居中。
fn initial_geometry(dpi: u32) -> (i32, i32, i32, i32) {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    let dpi = dpi.max(96);
    let scale = |v: i32| (v as f32 * dpi as f32 / 96.0).round() as i32;

    let w = scale(1000);
    let h = scale(700);
    let screen_w = unsafe { GetSystemMetrics(SM_CXSCREEN) };
    let screen_h = unsafe { GetSystemMetrics(SM_CYSCREEN) };

    let x = ((screen_w - w) / 2).max(0);
    let y = ((screen_h - h) / 3).max(0);
    (w, h, x, y)
}

/// 载入设置,并决定进入哪种形态。
///
/// 只有一个固定文件名的数据库:存在就进入解锁,不存在就走初始化创建。
fn setup_state() {
    let mode = if crate::paths::vault_exists() {
        Mode::Unlock
    } else {
        Mode::Create
    };

    app::set(app::AppState {
        // 设置加密存在库里,解锁前先用默认值。
        settings: Default::default(),
        vault: crate::vault::VaultService::new(),
        font: Default::default(),
        font_bold: Default::default(),
        dpi: 96,
        main: HWND::default(),
        mode,
    });
}

unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let dpi = app::state().dpi.max(96);
            let font = ui::create_ui_font(false, dpi);
            let font_bold = ui::create_ui_font(true, dpi);

            {
                let state = app::state();
                state.dpi = dpi;
                state.font = font;
                state.font_bold = font_bold;
                state.main = hwnd;
            }

            main_ui::on_create(hwnd, lparam);
            LRESULT(0)
        }
        WM_DPICHANGED => {
            // 窗口被移到不同 DPI 的显示器:重建字体,重新排版。
            let new_dpi = (wparam.0 & 0xFFFF) as u32;
            let state = app::state();
            ui::delete_font(state.font);
            ui::delete_font(state.font_bold);
            state.dpi = new_dpi.max(96);
            state.font = ui::create_ui_font(false, state.dpi);
            state.font_bold = ui::create_ui_font(true, state.dpi);

            main_ui::apply_fonts(hwnd);
            main_ui::layout(hwnd);
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            // 限制最小尺寸,避免窗口被拖小到布局挤成一团。
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let dpi = if app::is_ready() { app::state().dpi } else { 96 };
            let (min_w, min_h) = minimum_size(dpi);
            info.ptMinTrackSize = POINT {
                x: min_w,
                y: min_h,
            };
            LRESULT(0)
        }
        WM_SIZE => {
            main_ui::layout(hwnd);
            LRESULT(0)
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as usize;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u16;
            main_ui::on_command(hwnd, id, code);
            LRESULT(0)
        }
        WM_NOTIFY => {
            main_ui::on_notify(hwnd, lparam);
            LRESULT(0)
        }
        WM_CTLCOLORSTATIC => LRESULT(main_ui::on_ctlcolor_static(hwnd, wparam.0, lparam.0)),
        TSM_TAB_CHANGED => {
            main_ui::on_tab_changed(hwnd);
            LRESULT(0)
        }
        WM_CONTEXTMENU => {
            if !main_ui::on_context_menu(hwnd) {
                return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
            }
            LRESULT(0)
        }
        WM_TIMER => {
            if wparam.0 == TIMER_AUTOCLOSE {
                ui::kill_timer(hwnd, TIMER_AUTOCLOSE);
                ui::post_message(hwnd, WM_CLOSE, 0, 0);
            } else {
                main_ui::on_timer(hwnd, wparam.0);
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            if wparam.0 as u32 == WTS_SESSION_LOCK {
                main_ui::on_session_locked(hwnd);
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            if main_ui::confirm_exit(hwnd) {
                ui::destroy_window(hwnd);
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            main_ui::on_destroy();
            unsafe {
                PostQuitMessage(0);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
