//! 剪贴板读写(纯 Win32,不使用 OLE)。

use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use zeroize::Zeroizing;

const CF_UNICODETEXT: u32 = 13;
/// 单次读取的 UTF-16 码元上限,避免异常大的剪贴板块把 `String` 撑爆。
const MAX_TEXT_UNITS: usize = 1_000_000;

/// 「不进 Win+V 历史、不同步到云剪贴板」的三种标记格式,注册一次缓存复用。
///
/// 系统按格式名识别,数据是 DWORD 0 时排除对应行为。定时清空只清得掉剪贴板
/// 槽位本身,历史与云端的副本必须在写入这一步就别让它们进来。
fn exclusion_formats() -> &'static [u32] {
    static FORMATS: OnceLock<[u32; 3]> = OnceLock::new();
    FORMATS.get_or_init(|| {
        [
            "CanIncludeInClipboardHistory",
            "CanUploadToCloudClipboard",
            "ExcludeClipboardContentFromMonitorProcessing",
        ]
        .map(|name| {
            let wide: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) }
        })
    })
}

/// 给剪贴板放一个 DWORD 值(标记格式的约定值是 0)。
///
/// 刻意尽力而为:内容本身已经放好了,不该因为加固标记没放上就把「复制」
/// 判成失败。
fn set_dword(format: u32, value: u32) -> bool {
    if format == 0 {
        return false;
    }
    unsafe {
        let Ok(handle) = GlobalAlloc(GMEM_MOVEABLE, 4) else {
            return false;
        };
        let memory = HGLOBAL(handle.0);
        let ptr = GlobalLock(memory);
        if ptr.is_null() {
            let _ = GlobalFree(Some(memory));
            return false;
        }
        std::ptr::write(ptr as *mut u32, value);
        let _ = GlobalUnlock(memory);

        if SetClipboardData(format, Some(HANDLE(handle.0))).is_ok() {
            true
        } else {
            let _ = GlobalFree(Some(memory));
            false
        }
    }
}

/// 当前剪贴板内容是否带了全部排除标记(自检用):格式在,且值是 DWORD 0。
///
/// 只验「格式存在」不够 —— 值若写成 1 恰好是相反含义(明确要求加入历史 /
/// 云同步),那会得到「自检绿着、密码照样进 Win+V」的假绿。
pub fn has_exclusion_markers() -> bool {
    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return false;
        }
        let marked = exclusion_formats().iter().all(|format| {
            if *format == 0 {
                return false;
            }
            let Ok(handle) = GetClipboardData(*format) else {
                return false;
            };
            let memory = HGLOBAL(handle.0);
            if GlobalSize(memory) < 4 {
                return false;
            }
            let ptr = GlobalLock(memory) as *const u32;
            if ptr.is_null() {
                return false;
            }
            let value = *ptr;
            let _ = GlobalUnlock(memory);
            value == 0
        });
        let _ = CloseClipboard();
        marked
    }
}

pub fn set_text(text: &str) -> bool {
    // 排除格式的注册与剪贴板打开状态无关:先注册好,再进剪贴板会话。
    let formats = exclusion_formats();
    // 这个宽字符缓冲里是明文(很多时候就是密码),用完抹掉。
    // 注意:剪贴板本身仍会持有这段文字 —— 那是设计使然,靠自动清空处理。
    let wide = Zeroizing::new(
        text.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<u16>>(),
    );
    let bytes = wide.len() * 2;

    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return false;
        }

        let result = (|| -> bool {
            if EmptyClipboard().is_err() {
                return false;
            }

            let handle = match GlobalAlloc(GMEM_MOVEABLE, bytes) {
                Ok(h) => h,
                Err(_) => return false,
            };

            let memory = HGLOBAL(handle.0);
            let ptr = GlobalLock(memory);
            if ptr.is_null() {
                // 交不出去就必须自己释放,否则这块全局内存直接漏掉。
                let _ = GlobalFree(Some(memory));
                return false;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
            let _ = GlobalUnlock(memory);

            if SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))).is_ok() {
                // 文本已就位,再补上排除标记:密码不该在 Win+V 历史与云剪贴板里
                // 留下副本(自动清空只清得掉当前槽位)。
                for format in formats {
                    set_dword(*format, 0);
                }
                true
            } else {
                // 失败时系统没有接管这块内存,同样要自己释放。
                let _ = GlobalFree(Some(memory));
                false
            }
        })();

        let _ = CloseClipboard();
        result
    }
}

/// 清空剪贴板。
pub fn clear() {
    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        let _ = CloseClipboard();
    }
}

/// 读取剪贴板的文本。返回值用 `Zeroizing` 包住 —— 内容常常就是刚复制的密码,
/// 比对完不该在堆上留副本。
pub fn get_text() -> Option<Zeroizing<String>> {
    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return None;
        }

        let result = (|| -> Option<Zeroizing<String>> {
            let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
            let memory = HGLOBAL(handle.0);

            // 这块内存是别的进程放上来的,长度由它说了算 —— 必须先按
            // 系统报告的真实大小设界,只靠"扫到 0 为止"会越过分配往外读。
            let bytes = GlobalSize(memory);
            if bytes < 2 {
                return None;
            }
            let cap = (bytes / 2).min(MAX_TEXT_UNITS);

            let ptr = GlobalLock(memory) as *const u16;
            if ptr.is_null() {
                return None;
            }

            let mut len = 0usize;
            while len < cap && *ptr.add(len) != 0 {
                len += 1;
            }
            let text = Zeroizing::new(String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len)));
            let _ = GlobalUnlock(memory);
            Some(text)
        })();

        let _ = CloseClipboard();
        result
    }
}
