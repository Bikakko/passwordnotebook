//! Windows Hello(指纹 / 人脸 / PIN)验证。
//!
//! 两个关键点,都是踩过坑的:
//!
//! 1. 桌面程序必须用 **`IUserConsentVerifierInterop::RequestVerificationForWindowAsync`**
//!    并传入自己的窗口句柄 —— 否则验证框没有属主,不会被激活,得用鼠标点一下切过去。
//! 2. 这个调用**必须在能泵消息的 UI 线程(STA)上发起**。放到后台线程会死锁:
//!    它内部要等对话框的消息,而后台线程永远不会泵消息。
//!
//! 所以这里的设计是:UI 线程**发起**(立即返回一个异步对象),再用定时器**轮询**结果,
//! 全程不阻塞消息循环。

use std::cell::RefCell;

use windows::core::{factory, HSTRING};
use windows::Security::Credentials::UI::{
    UserConsentVerificationResult, UserConsentVerifier, UserConsentVerifierAvailability,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::System::WinRT::IUserConsentVerifierInterop;
use windows_future::{AsyncStatus, IAsyncOperation};

thread_local! {
    /// 正在等待结果的验证请求。只在 UI 线程上访问。
    static PENDING: RefCell<Option<IAsyncOperation<UserConsentVerificationResult>>> =
        const { RefCell::new(None) };
}

/// 此设备是否可用 Windows Hello(可在后台线程调用,不会弹界面)。
pub fn is_available() -> bool {
    std::thread::spawn(|| {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        UserConsentVerifier::CheckAvailabilityAsync()
            .ok()
            .map(|op| {
                for _ in 0..40 {
                    match op.Status() {
                        Ok(AsyncStatus::Completed) => {
                            return op
                                .GetResults()
                                .map(|a| a == UserConsentVerifierAvailability::Available)
                                .unwrap_or(false);
                        }
                        Ok(AsyncStatus::Error) | Ok(AsyncStatus::Canceled) => return false,
                        _ => std::thread::sleep(std::time::Duration::from_millis(25)),
                    }
                }
                false
            })
            .unwrap_or(false)
    })
    .join()
    .unwrap_or(false)
}

/// 发起验证请求。**必须在 UI 线程调用**;成功发起返回 true(界面由系统弹出)。
pub fn request(owner: HWND, message: &str) -> bool {
    let text = HSTRING::from(message);

    let Ok(interop) = factory::<UserConsentVerifier, IUserConsentVerifierInterop>() else {
        return false;
    };

    let started: Result<IAsyncOperation<UserConsentVerificationResult>, _> =
        unsafe { interop.RequestVerificationForWindowAsync(owner, &text) };

    match started {
        Ok(operation) => {
            PENDING.with(|slot| *slot.borrow_mut() = Some(operation));
            true
        }
        Err(_) => false,
    }
}

/// 轮询验证结果。`None` 表示还没结束;`Some(true/false)` 表示已有结论。
///
/// 在 UI 线程的定时器里调用。
pub fn poll() -> Option<bool> {
    PENDING.with(|slot| {
        let mut pending = slot.borrow_mut();
        let operation = pending.as_ref()?;

        match operation.Status() {
            Ok(AsyncStatus::Completed) => {
                let verified = operation
                    .GetResults()
                    .map(|r| r == UserConsentVerificationResult::Verified)
                    .unwrap_or(false);
                *pending = None;
                Some(verified)
            }
            Ok(AsyncStatus::Error) | Ok(AsyncStatus::Canceled) => {
                *pending = None;
                Some(false)
            }
            _ => None,
        }
    })
}
