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
pub mod tokens;
pub mod ui;

mod dialog;
mod dlg_editor;
mod dlg_generator;
mod dlg_input;
mod dlg_recovery;
mod dlg_settings;
mod dlg_taxonomy;
mod dlg_transfer;
mod main_ui;
mod main_window;
mod selftest;
mod window_state;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, ICC_LISTVIEW_CLASSES, ICC_TAB_CLASSES, INITCOMMONCONTROLSEX,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::FindWindowW;

use sys::SW_RESTORE;
use ui::Wz;

const SINGLE_INSTANCE_MUTEX: &str = "PasswordNotebook.SingleInstance.v1";

/// 程序名。
pub const APP_NAME: &str = "PasswordNotebook";
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

        let tabs = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_TAB_CLASSES,
        };
        let _ = InitCommonControlsEx(&tabs);

        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
    }

    if selftest::requested() {
        return selftest::run();
    }

    if selftest::preview_requested() {
        return selftest::preview();
    }

    if selftest::preview_recovery_requested() {
        return selftest::preview_recovery();
    }

    if selftest::preview_transfer_requested() {
        return selftest::preview_transfer();
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

/// 自检模式的控制台状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ConsoleState {
    /// 输出有去处:挂到了父进程的控制台,或标准输出本就被重定向到文件/管道。
    Ready,
    /// 父进程没有控制台,我们自己开了一个窗口 —— 它随进程一起消失。
    Allocated,
    /// 拿不到任何控制台:调用方应当把结果写到文件并弹框告诉用户。
    Missing,
}

/// 程序是 GUI 子系统,默认没有控制台。自检模式把输出接回终端,
/// 让 `pnb.exe --selftest` 能在 cmd / PowerShell 里正常显示。
///
/// 必须在任何 `println!` **之前**调用 —— Rust 会缓存标准输出的句柄。
///
/// 这里踩过一个坑:命令行启动 GUI 程序时,系统会把父进程的标准句柄**原样继承**
/// 过来。那个句柄值非空,但进程并没有附到那个控制台上,直接写会失败 ——
/// 而 Rust 的 `println!` 写失败会 panic,release 下 `panic = "abort"`,
/// 于是整个进程立刻消失,用户只看到「一闪就没了」。
/// 所以判断依据不能是「句柄非空」,而必须是「句柄指向的是文件/管道」。
pub fn prepare_console() -> ConsoleState {
    use windows::Win32::System::Console::GetConsoleWindow;

    unsafe {
        // 已经被重定向到文件或管道:不要动它,否则会把用户指定的输出目标覆盖掉。
        if stdout_is_redirected() {
            return ConsoleState::Ready;
        }

        // 已经有控制台(例如在调试器里启动)就直接用。
        if !GetConsoleWindow().is_invalid() {
            return ConsoleState::Ready;
        }

        if windows::Win32::System::Console::AttachConsole(
            windows::Win32::System::Console::ATTACH_PARENT_PROCESS,
        )
        .is_ok()
        {
            bind_console_handles();
            return ConsoleState::Ready;
        }

        // 父进程没有控制台(双击启动、或被 IDE 拉起):自己开一个。
        // 否则输出会写到上面那个「看着有效其实不可用」的句柄上。
        if windows::Win32::System::Console::AllocConsole().is_ok() {
            bind_console_handles();
            // 自己开的窗口,顺手把它调成能显示中文的样子。
            set_utf8_code_page();
            ensure_console_font();
            return ConsoleState::Allocated;
        }

        ConsoleState::Missing
    }
}

/// 标准输出是否被重定向到了文件或管道(区别于控制台字符设备)。
fn stdout_is_redirected() -> bool {
    use windows::Win32::Storage::FileSystem::{FILE_TYPE_CHAR, GetFileType};
    use windows::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE};

    unsafe {
        let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) else {
            return false;
        };
        if handle.is_invalid() {
            return false;
        }
        // 控制台是字符设备;文件是 FILE_TYPE_DISK,管道是 FILE_TYPE_PIPE。
        GetFileType(handle) != FILE_TYPE_CHAR
    }
}

/// 当前控制台的文本信息,给自检输出用。
///
/// 中文在控制台里显示成乱码时,只有两个原因:输出代码页不是 UTF-8,
/// 或者窗口字体没有汉字字形。把这两项打出来,下次就不用猜了。
pub struct ConsoleInfo {
    pub output_code_page: u32,
    pub font_face: String,
    pub font_size_y: i16,
    pub font_family: u32,
}

/// 读取当前控制台信息;标准输出不是控制台时返回 `None`。
pub fn console_info() -> Option<ConsoleInfo> {
    use windows::Win32::Storage::FileSystem::{FILE_TYPE_CHAR, GetFileType};
    use windows::Win32::System::Console::{
        CONSOLE_FONT_INFOEX, GetConsoleOutputCP, GetCurrentConsoleFontEx, GetStdHandle,
        STD_OUTPUT_HANDLE,
    };

    unsafe {
        let handle = GetStdHandle(STD_OUTPUT_HANDLE).ok()?;
        if handle.is_invalid() || GetFileType(handle) != FILE_TYPE_CHAR {
            return None;
        }

        let mut font = CONSOLE_FONT_INFOEX {
            cbSize: std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32,
            ..Default::default()
        };
        let has_font = GetCurrentConsoleFontEx(handle, false, &mut font).is_ok();

        let face = if has_font {
            let end = font.FaceName.iter().position(|&c| c == 0).unwrap_or(font.FaceName.len());
            String::from_utf16_lossy(&font.FaceName[..end])
        } else {
            "(未知)".to_string()
        };

        Some(ConsoleInfo {
            output_code_page: GetConsoleOutputCP(),
            font_face: face,
            font_size_y: font.dwFontSize.Y,
            font_family: font.FontFamily,
        })
    }
}

/// 把标准输出/错误/输入接到当前控制台上。
fn bind_console_handles() {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::Console::{
        SetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
    };

    unsafe {
        if let Ok(file) = std::fs::OpenOptions::new().write(true).open("CONOUT$") {
            let handle = HANDLE(file.as_raw_handle() as _);
            let _ = SetStdHandle(STD_OUTPUT_HANDLE, handle);
            let _ = SetStdHandle(STD_ERROR_HANDLE, handle);
            // 句柄必须一直有效,不能在这里关闭。
            std::mem::forget(file);
        }

        // 输入也要接上:自检窗口需要「按回车键关闭」。
        if let Ok(file) = std::fs::OpenOptions::new().read(true).open("CONIN$") {
            let handle = HANDLE(file.as_raw_handle() as _);
            let _ = SetStdHandle(STD_INPUT_HANDLE, handle);
            std::mem::forget(file);
        }
    }
}

/// 把控制台代码页设为 UTF-8。
///
/// **只在自己开出来的窗口上做**:附加到用户已有终端时改代码页会影响到他后续的命令,
/// 而本程序的中文输出已经走 [`write_console`] 的宽字符接口,本来就不依赖代码页。
fn set_utf8_code_page() {
    use windows::Win32::System::Console::{SetConsoleCP, SetConsoleOutputCP};

    /// UTF-8 代码页(`CP_UTF8`);为拿一个常量启用 Win32_Globalization 不划算。
    const CP_UTF8: u32 = 65001;

    unsafe {
        let _ = SetConsoleOutputCP(CP_UTF8);
        let _ = SetConsoleCP(CP_UTF8);
    }
}

/// 把一行文字直接写到控制台(UTF-16,`WriteConsoleW`)。
///
/// 相比让 Rust 的 stdout 写 UTF-8 字节,这条路**完全绕开控制台代码页**:
/// 字节写法的解码方式取决于代码页,代码页没改成功(或用户手工改回去)时中文就是乱码。
/// 返回 `false` 表示标准输出不是控制台(文件/管道)或写入失败,调用方应回退到字节写入。
pub fn write_console(text: &str) -> bool {
    use windows::Win32::Storage::FileSystem::{FILE_TYPE_CHAR, GetFileType};
    use windows::Win32::System::Console::{GetStdHandle, STD_OUTPUT_HANDLE, WriteConsoleW};

    unsafe {
        let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) else {
            return false;
        };
        if handle.is_invalid() || GetFileType(handle) != FILE_TYPE_CHAR {
            return false;
        }

        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.extend_from_slice(&[b'\r' as u16, b'\n' as u16]);
        WriteConsoleW(handle, &wide, None, None).is_ok()
    }
}

/// 我们自己开出来的控制台窗口:确保用的是带中文字形的 TrueType 字体。
///
/// `AllocConsole` 用的默认字体在某些系统上是不含汉字的点阵字体,中文会显示成方块。
/// 只在自己开的窗口上做这件事 —— 绝不改用户已有终端的字体设置。
fn ensure_console_font() {
    use windows::Win32::System::Console::{
        CONSOLE_FONT_INFOEX, COORD, GetStdHandle, STD_OUTPUT_HANDLE, SetCurrentConsoleFontEx,
    };

    // 按优先级尝试:前几个都带中文字形,Consolas 只作为最后兜底。
    const CANDIDATES: [&str; 4] = ["Microsoft YaHei Mono", "NSimSun", "SimSun", "Consolas"];
    // 字号也要试:某些字体不支持指定字号时调用会直接失败。
    const SIZES: [i16; 2] = [18, 16];

    unsafe {
        let Ok(handle) = GetStdHandle(STD_OUTPUT_HANDLE) else {
            return;
        };
        if handle.is_invalid() {
            return;
        }

        for size in SIZES {
            for face in CANDIDATES {
                let mut font = CONSOLE_FONT_INFOEX {
                    cbSize: std::mem::size_of::<CONSOLE_FONT_INFOEX>() as u32,
                    dwFontSize: COORD { X: 0, Y: size },
                    FontWeight: 400,
                    ..Default::default()
                };
                for (slot, unit) in font.FaceName.iter_mut().zip(face.encode_utf16()) {
                    *slot = unit;
                }
                // 字体不存在时调用会失败,换下一个;成功就收工。
                if SetCurrentConsoleFontEx(handle, false, &font).is_ok() {
                    return;
                }
            }
        }
    }
}
