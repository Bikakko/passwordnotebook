//! 模态对话框框架。

use std::ffi::c_void;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsDialogMessageW, IsWindow, MSG, TranslateMessage,
};

use super::sys::*;
use super::ui::{self, WndProc};

/// 运行对话框自己的消息循环,直到该窗口被销毁。
pub fn run_modal(hwnd: HWND) {
    let mut msg = MSG::default();
    while unsafe { IsWindow(Some(hwnd)).as_bool() } {
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        unsafe {
            // IsDialogMessageW 负责 Tab 切换焦点、回车触发默认按钮。
            if !IsDialogMessageW(hwnd, &msg).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
    }
}

/// 相对所有者窗口居中的左上角坐标。
pub fn centered_position(owner: HWND, w: i32, h: i32) -> (i32, i32) {
    if owner.is_invalid() {
        return (0, 0);
    }
    let (ox, oy, ow, oh) = ui::window_rect(owner);
    (ox + (ow - w).max(0) / 2, oy + (oh - h).max(0) / 2)
}

/// 打开一个模态对话框,返回其状态(窗口销毁后归还所有权)。
pub fn open<T>(
    class: &str,
    title: &str,
    owner: HWND,
    proc: WndProc,
    state: Box<T>,
    width: i32,
    height: i32,
) -> Box<T> {
    let _ = ui::register_class(class, proc);

    let dpi = if owner.is_invalid() {
        96
    } else {
        ui::window_dpi(owner)
    };
    let scale = |v: i32| (v as f32 * dpi as f32 / 96.0).round() as i32;
    let w = scale(width);
    let h = scale(height);
    let (x, y) = centered_position(owner, w, h);

    let ptr = Box::into_raw(state);
    let hwnd = ui::create_window_with_param(
        class,
        title,
        DIALOG_WINDOW | WS_CLIPCHILDREN,
        WS_EX_DLGMODALFRAME | WS_EX_CONTROLPARENT,
        owner,
        0,
        x,
        y,
        w,
        h,
        ptr as *const c_void,
    );

    if hwnd.is_invalid() {
        return unsafe { Box::from_raw(ptr) };
    }

    if !owner.is_invalid() {
        ui::enable(owner, false);
    }

    ui::show_window(hwnd, SW_SHOW);
    ui::update_window(hwnd);
    run_modal(hwnd);

    if !owner.is_invalid() {
        ui::enable(owner, true);
        ui::set_foreground(owner);
    }

    unsafe { Box::from_raw(ptr) }
}
