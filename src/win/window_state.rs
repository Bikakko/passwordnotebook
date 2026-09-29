//! 窗口位置与状态的持久化(存注册表)。
//!
//! 存到 `HKCU\Software\PasswordNotebook\WindowPlacement`,值是 `WINDOWPLACEMENT`
//! 结构的原始字节 —— 位置、尺寸、最大化状态一次带走。
//!
//! 为什么放注册表而不是放进加密的数据库:窗口在**解锁之前**就要创建,
//! 那时库还读不到。放注册表才能做到"打开就是上次的样子",且**不在磁盘上产生文件**。
//! 窗口形状不是秘密,这里也不涉及任何敏感信息。

use std::ffi::c_void;

use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
    KEY_QUERY_VALUE, KEY_SET_VALUE, REG_BINARY, REG_OPTION_NON_VOLATILE, REG_VALUE_TYPE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowPlacement, SetWindowPlacement, WINDOWPLACEMENT,
};

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
        Some(placement)
    }
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

/// 让 `c_void` 的导入不被判为未使用(本模块只用类型名)。
#[allow(dead_code)]
type UnusedPtr = *mut c_void;
