//! 窗口位置与状态的持久化(存注册表)。
//!
//! 存到 `HKCU\Software\PasswordNotebook\WindowPlacement`,值是 `WINDOWPLACEMENT`
//! 结构的原始字节 —— 位置、尺寸、最大化状态一次带走。
//!
//! 为什么放注册表而不是放进加密的数据库:窗口在**解锁之前**就要创建,
//! 那时库还读不到。放注册表才能做到"打开就是上次的样子",且**不在磁盘上产生文件**。
//! 窗口形状不是秘密,这里也不涉及任何敏感信息。

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, SetWindowPlacement, WINDOWPLACEMENT,
};

use super::sys::{SW_SHOWMAXIMIZED, SW_SHOWMINIMIZED, SW_SHOWNORMAL};
use super::ui::Wz;

const SUBKEY: &str = "Software\\PasswordNotebook";
const VALUE_NAME: &str = "WindowPlacement";

fn placement_size() -> usize {
    std::mem::size_of::<WINDOWPLACEMENT>()
}

/// 读取上次保存的窗口状态;不存在或格式不符则返回 None。
pub fn load() -> Option<WINDOWPLACEMENT> {
    unsafe {
        let subkey = Wz::new(SUBKEY);
        let mut key = HKEY::default();
        let created = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.pcwstr(),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_QUERY_VALUE,
            None,
            &mut key,
            None,
        );
        if created.0 != 0 {
            return None;
        }

        let name = Wz::new(VALUE_NAME);
        let mut size: u32 = 0;
        let probed = RegQueryValueExW(key, name.pcwstr(), None, None, None, Some(&mut size));
        if probed.0 != 0 || size as usize != placement_size() {
            let _ = RegCloseKey(key);
            return None;
        }

        let mut placement = WINDOWPLACEMENT::default();
        let mut value_type = REG_VALUE_TYPE::default();
        let read = RegQueryValueExW(
            key,
            name.pcwstr(),
            None,
            Some(&mut value_type),
            Some(&mut placement as *mut _ as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);

        // 长度字段对不上说明是旧版本写的,直接不认(会退回默认几何)。
        if read.0 != 0 || placement.length as usize != placement_size() {
            return None;
        }

        // showCmd 也得是认识的取值。这份状态存在 HKCU,可能被外部改坏、或在写入
        // 中断时残缺;而 showCmd 一旦是 0(SW_HIDE)或别的野值,restore/ShowWindow
        // 会让窗口一启动就是隐藏的 —— 用户以为程序根本没打开,又因为没有可见窗口
        // 而找不到任务栏按钮。不认它,退回默认几何 + 正常显示。
        if !show_cmd_is_valid(placement.showCmd) {
            return None;
        }
        Some(placement)
    }
}

/// `GetWindowPlacement` 只会产出这三种 `showCmd`;其余一律视为损坏。
pub(crate) fn show_cmd_is_valid(show_cmd: u32) -> bool {
    matches!(
        show_cmd as i32,
        SW_SHOWNORMAL | SW_SHOWMINIMIZED | SW_SHOWMAXIMIZED
    )
}

/// 保存当前窗口状态。
pub fn save(placement: &WINDOWPLACEMENT) {
    unsafe {
        let subkey = Wz::new(SUBKEY);
        let mut key = HKEY::default();
        let created = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.pcwstr(),
            None,
            PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut key,
            None,
        );
        if created.0 != 0 {
            return;
        }

        let bytes = std::slice::from_raw_parts(
            placement as *const WINDOWPLACEMENT as *const u8,
            placement_size(),
        );
        let name = Wz::new(VALUE_NAME);
        let _ = RegSetValueExW(key, name.pcwstr(), None, REG_BINARY, Some(bytes));
        let _ = RegCloseKey(key);
    }
}

/// 取窗口当前状态(关闭前调用)。
pub fn capture(hwnd: HWND) -> Option<WINDOWPLACEMENT> {
    let mut placement = WINDOWPLACEMENT {
        length: placement_size() as u32,
        ..Default::default()
    };
    unsafe {
        GetWindowPlacement(hwnd, &mut placement).ok()?;
    }
    Some(placement)
}

/// 把保存的状态应用到窗口。
pub fn restore(hwnd: HWND, placement: &WINDOWPLACEMENT) -> bool {
    let mut placement = *placement;
    placement.length = placement_size() as u32;
    unsafe { SetWindowPlacement(hwnd, &placement).is_ok() }
}
