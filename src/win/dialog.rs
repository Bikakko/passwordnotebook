//! 模态对话框框架。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, IsDialogMessageW, IsWindow, MSG, TranslateMessage,
};

use super::sys::*;
use super::ui::{self, WndProc};

/// 当前打开的模态对话框层数。对话框可以嵌套(如编辑条目里再开密码生成器),
/// 所以用计数而非布尔。
static MODAL_DEPTH: AtomicU32 = AtomicU32::new(0);

/// 是否有模态对话框正开着。
///
/// 空闲自动锁定据此让路:对话框开着时把库锁掉,对话框还在、随后的保存必然
/// 失败,用户填了一半的内容就白填了。
pub fn is_modal_open() -> bool {
    MODAL_DEPTH.load(Ordering::Relaxed) > 0
}

/// 运行对话框自己的消息循环,直到该窗口被销毁。
pub fn run_modal(hwnd: HWND) {
    run_modal_with(hwnd, |_| false);
}

/// 同上,但每条消息先交给 `intercept` 看一眼;它返回 true 表示已处理、不再派发。
///
/// 主窗口用它接住回车:这些自定义窗口类不归对话框管理器管,`IsDialogMessageW`
/// 的「回车触发默认按钮」对它们不生效(详见 `main_ui::intercept_key`)。
pub fn run_modal_with(hwnd: HWND, mut intercept: impl FnMut(&MSG) -> bool) {
    let mut msg = MSG::default();
    while unsafe { IsWindow(Some(hwnd)).as_bool() } {
        let result = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if result.0 <= 0 {
            break;
        }
        if intercept(&msg) {
            continue;
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

    MODAL_DEPTH.fetch_add(1, Ordering::Relaxed);
    run_modal(hwnd);
    MODAL_DEPTH.fetch_sub(1, Ordering::Relaxed);

    if !owner.is_invalid() {
        ui::enable(owner, true);
        ui::set_foreground(owner);
    }

    unsafe { Box::from_raw(ptr) }
}
