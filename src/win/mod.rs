//! Windows 原生层入口。
//!
//! 退出路径是刻意设计的:唯一的顶层窗口被销毁 → `WM_DESTROY` → `PostQuitMessage`
//! → `GetMessageW` 返回 0 → 消息循环退出 → `run()` 返回 → 进程结束。
//! 全程没有后台线程持有,所以关闭窗口后进程会立即消失。

#![allow(non_snake_case)]

pub mod app;
pub mod clipboard;
pub mod dpapi;
pub mod hello;
pub mod idle;
pub mod sys;
pub mod timefmt;
pub mod ui;

mod dialog;
mod dlg_editor;
mod dlg_generator;
mod dlg_recovery;
mod dlg_settings;
mod dlg_taxonomy;
mod main_ui;
mod main_window;
mod selftest;
mod window_state;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_LISTVIEW_CLASSES};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

use sys::SW_RESTORE;
use ui::Wz;

const SINGLE_INSTANCE_MUTEX: &str = "PasswordNotebook.SingleInstance.v1";

/// 程序名。
pub const APP_NAME: &str = "我的密码本";
/// 版本号,来自 Cargo.toml。
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// 标题里带上版本号,界面上始终能看到在跑哪个版本。
pub fn app_title() -> String {
    format!("{APP_NAME} {APP_VERSION}")
}

pub fn run() -> i32 {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);

        let controls = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        let _ = InitCommonControlsEx(&controls);

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    if selftest::requested() {
        return selftest::run();
    }

    // 单实例:第二次启动时把已有窗口拉到前台,避免两个进程同时占着密码本文件。
    if already_running() {
        if let Some(existing) = find_main_window() {
            ui::show_window(existing, SW_RESTORE);
            ui::set_foreground(existing);
        }
        return 0;
    }

    main_window::run_main()
}

fn already_running() -> bool {
    let name = Wz::new(SINGLE_INSTANCE_MUTEX);
    let Ok(handle) = (unsafe { CreateMutexW(None, true, name.pcwstr()) }) else {
        return false;
    };

    if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
        // 已有实例持有该互斥量,关掉我们这份多余句柄。
        unsafe {
            let _ = CloseHandle(handle);
        }
        true
    } else {
        // 刻意不关闭句柄:它作为单实例锁一直持有到本进程退出,由系统回收。
        false
    }
}

fn find_main_window() -> Option<HWND> {
    let class = Wz::new(main_window::CLASS_NAME);
    unsafe { FindWindowW(class.pcwstr(), PCWSTR::null()) }.ok()
}

/// 程序是 GUI 子系统,默认没有控制台。自检模式挂到父进程的控制台上,
/// 这样在终端里运行 `--selftest` 时仍能看到输出。
///
/// 必须在任何 `println!` **之前**调用 —— Rust 会缓存标准输出的句柄。
pub fn attach_parent_console() {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
        STD_OUTPUT_HANDLE,
    };

    unsafe {
        // 若标准输出已经指向某个有效目标(例如被重定向到文件或管道),就不要动它 ——
        // 否则会把已有的输出目标覆盖掉,反而什么都看不到。
        if let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) {
            if !handle.is_invalid() {
                return;
            }
        }

        // 双击启动时没有控制台;从终端启动时挂到父进程的控制台上。
        if AttachConsole(ATTACH_PARENT_PROCESS).is_err() {
            return;
        }

        if let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let handle = HANDLE(file.as_raw_handle() as _);
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
            let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
            // 句柄必须一直有效,不能在这里关闭。
            std::mem::forget(file);
        }
    }
}
