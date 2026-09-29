//! 时间格式化(UTC 秒 → 本地时间字符串),使用 Win32 处理时区与夏令时。

use windows::Win32::Foundation::{FILETIME, SYSTEMTIME};
use windows::Win32::Storage::FileSystem::FileTimeToLocalFileTime;
use windows::Win32::System::Time::FileTimeToSystemTime;

/// Unix 纪元(1970-01-01)与 Windows 纪元(1601-01-01)之间相差的秒数。
const EPOCH_DIFF_SECS: i64 = 11_644_473_600;

pub fn local_string(unix_secs: i64) -> String {
    let st = match to_local_system_time(unix_secs) {
        Some(st) => st,
        None => return String::new(),
    };

    if st.wYear == 1601 {
        // 转换失败时的占位。
        return "(未知时间)".to_string();
    }

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}",
        st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute
    )
}

pub fn local_date_string(unix_secs: i64) -> String {
    let st = match to_local_system_time(unix_secs) {
        Some(st) => st,
        None => return String::new(),
    };
    format!("{:04}-{:02}-{:02}", st.wYear, st.wMonth, st.wDay)
}

fn to_local_system_time(unix_secs: i64) -> Option<SYSTEMTIME> {
    let intervals = (unix_secs + EPOCH_DIFF_SECS).checked_mul(10_000_000)?;
    if intervals < 0 {
        return None;
    }

    let utc = FILETIME {
        dwLowDateTime: (intervals as u64 & 0xFFFF_FFFF) as u32,
        dwHighDateTime: ((intervals as u64) >> 32) as u32,
    };

    unsafe {
        let mut local = FILETIME::default();
        FileTimeToLocalFileTime(&utc, &mut local).ok()?;

        let mut st = SYSTEMTIME::default();
        FileTimeToSystemTime(&local, &mut st).ok()?;
        Some(st)
    }
}

/// 当前 Unix 秒。
pub fn now() -> i64 {
    crate::model::now_secs()
}
