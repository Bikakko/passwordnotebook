//! 全局应用状态。
//!
//! 所有访问都发生在 UI 线程上,因此这里用一个 `UnsafeCell` 包住的全局单例,
//! 换取「随时可取 &mut 状态」的便利(避免 RefCell 在重入时 panic)。

use std::cell::UnsafeCell;

use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::HFONT;

use crate::model::Settings;
use crate::vault::VaultService;

pub struct AppState {
    pub settings: Settings,
    pub vault: VaultService,
    pub font: HFONT,
    pub font_bold: HFONT,
    /// 图标字体(空状态大图标 / 搜索框放大镜);系统缺图标字体时为空句柄。
    pub font_icon_lg: HFONT,
    pub font_icon_sm: HFONT,
    pub dpi: u32,
    pub main: HWND,
    /// 主窗口当前处于哪种形态。
    pub mode: Mode,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// 解锁已有密码本。
    Unlock,
    /// 创建新密码本。
    Create,
    /// 已解锁,显示条目列表。
    Unlocked,
    /// 回收站。
    Bin,
}

struct Global(UnsafeCell<Option<AppState>>);

// 安全说明:本程序是单线程 GUI,所有访问都在同一个 UI 线程内完成。
unsafe impl Sync for Global {}

static GLOBAL: Global = Global(UnsafeCell::new(None));

pub fn set(state: AppState) {
    unsafe {
        *GLOBAL.0.get() = Some(state);
    }
}

pub fn is_ready() -> bool {
    unsafe { (*GLOBAL.0.get()).is_some() }
}

/// 取得全局状态的可变引用(仅允许在 UI 线程调用)。
///
/// 与 `ui::state_ref` 同理:返回的是 `&'static mut`,再调一次就与上一把互为别名。
/// 单线程并不使别名合法 —— 不要把结果存进局部变量后跨过可能重入的调用
/// (定时器、模态对话框、`MessageBox`)继续使用;需要就"用时重新取"。
#[allow(clippy::mut_from_ref)]
pub fn state() -> &'static mut AppState {
    unsafe {
        (*GLOBAL.0.get())
            .as_mut()
            .expect("应用状态尚未初始化")
    }
}
