//! 剪贴板读写(纯 Win32,不使用 OLE)。

use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use zeroize::Zeroizing;

const CF_UNICODETEXT: u32 = 13;
/// 单次读取的 UTF-16 码元上限,避免异常大的剪贴板块把 `String` 撑爆。
const MAX_TEXT_UNITS: usize = 1_000_000;

pub fn set_text(text: &str) -> bool {
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
                return false;
            }
            std::ptr::copy_nonoverlapping(wide.as_ptr() as *const u8, ptr as *mut u8, bytes);
            let _ = GlobalUnlock(memory);

            SetClipboardData(CF_UNICODETEXT, Some(HANDLE(handle.0))).is_ok()
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

pub fn get_text() -> Option<String> {
    unsafe {
        if OpenClipboard(Some(HWND::default())).is_err() {
            return None;
        }

        let result = (|| -> Option<String> {
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
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
            let _ = GlobalUnlock(memory);
            Some(text)
        })();

        let _ = CloseClipboard();
        result
    }
}
