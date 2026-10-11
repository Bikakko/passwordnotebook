//! 空闲时间与屏保状态查询。

use windows::Win32::System::SystemInformation::GetTickCount64;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    SystemParametersInfoW, SYSTEM_PARAMETERS_INFO_ACTION, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use super::sys::{SPI_GETSCREENSAVEACTIVE, SPI_GETSCREENSAVETIMEOUT};

/// 距离最后一次键鼠输入的秒数;拿不到输入信息时返回 `None`。
///
/// 失败方向刻意是 fail-closed:调用方(空闲自动锁定)把 `None` 当作
/// 「已空闲」处理 —— 安全看门狗宁可误锁一次,也不能因为检测失败而永不锁定。
pub fn idle_seconds() -> Option<u64> {
    unsafe {
        let mut info = LASTINPUTINFO {
            cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32,
            dwTime: 0,
        };
        if !GetLastInputInfo(&mut info).as_bool() {
            return None;
        }

        // dwTime 只有 32 位;和 64 位计数器取相同低位后比较,可正确处理回绕。
        let now = GetTickCount64() & 0xFFFF_FFFF;
        let last = info.dwTime as u64;
        let elapsed_ms = if now >= last {
            now - last
        } else {
            (0x1_0000_0000 - last) + now
        };
        Some(elapsed_ms / 1000)
    }
}

pub fn screen_saver_active() -> bool {
    let mut active: i32 = 0;
    unsafe {
        SystemParametersInfoW(
            SYSTEM_PARAMETERS_INFO_ACTION(SPI_GETSCREENSAVEACTIVE),
            0,
            Some(&mut active as *mut i32 as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
            && active != 0
    }
}

/// 系统屏保等待秒数;未启用屏保时返回 0。
pub fn screen_saver_timeout_seconds() -> u32 {
    let mut timeout: i32 = 0;
    unsafe {
        let ok = SystemParametersInfoW(
            SYSTEM_PARAMETERS_INFO_ACTION(SPI_GETSCREENSAVETIMEOUT),
            0,
            Some(&mut timeout as *mut i32 as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if ok.is_ok() && timeout > 0 {
            timeout as u32
        } else {
            0
        }
    }
}
