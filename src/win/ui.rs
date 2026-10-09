//! Win32 控件与窗口的薄封装。
//!
//! 这里把 `windows` crate 的新类型(各种 `XXX_STYLE`、`SHOW_WINDOW_CMD` 等)
//! 全部消化掉,调用方只需要普通的 u32 / i32,以及 [`sys`](super::sys) 里的常量。

use std::ffi::c_void;
use std::sync::{Mutex, OnceLock};

use windows::core::{BOOL, PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, CreateSolidBrush, DeleteObject, GetSysColorBrush, InvalidateRect,
    RedrawWindow, ScreenToClient, SetBkColor, SetBkMode, SetTextColor, UpdateWindow, BACKGROUND_MODE,
    CLEARTYPE_QUALITY, COLOR_WINDOW, HBRUSH, HDC, HFONT, HGDIOBJ, LOGFONTW,
    RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_FLAGS, LIST_VIEW_ITEM_STATE_FLAGS, LVCFMT_LEFT, LVCOLUMNW, LVCOLUMNW_FORMAT,
    LVCOLUMNW_MASK, LVCF_FMT, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVIS_FOCUSED, LVIS_SELECTED,
    LVITEMW,
};
use windows::Win32::UI::Shell::SetWindowSubclass;
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DestroyMenu, DestroyWindow, EnumChildWindows,
    GetClassNameW, GetCursorPos, GetDlgItem, GetWindowLongPtrW, GetWindowTextLengthW,
    GetWindowTextW, HMENU, IDC_ARROW,
    HICON, KillTimer, LoadCursorW, LoadIconW, MESSAGEBOX_STYLE, MessageBoxW, MF_GRAYED,
    MF_POPUP,
    MF_SEPARATOR, MF_STRING,
    NONCLIENTMETRICSW, PostMessageW, RegisterClassW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
    SystemParametersInfoW, TrackPopupMenu, CS_HREDRAW, CS_VREDRAW, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_NULL, WNDCLASSW, WNDCLASS_STYLES,
    DefWindowProcW,
    GetParent,
    WM_PAINT,
    WM_LBUTTONDOWN,
    WM_SETFONT,
    WM_ERASEBKGND,
};

use zeroize::Zeroizing;

use super::sys::*;
use super::tokens::*;

pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// 子类化过程的函数指针类型(与 `SetWindowSubclass` 的形参一致)。
pub type SubclassProc =
    unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM, usize, usize) -> LRESULT;

/// UTF-16 缓冲区,保证在 CreateWindowExW / RegisterClassW 调用期间指针有效。
/// 宽字符缓冲。
///
/// 一律包一层 `Zeroizing`:被写进控件或消息框的文字里可能是密码
/// (例如打开条目编辑器时把密码填进输入框),在这一层统一抹掉,
/// 免得每个调用方各自记得处理 —— 读路径和写路径必须对称。
pub struct Wz(Zeroizing<Vec<u16>>);

impl Wz {
    pub fn new(s: &str) -> Self {
        Self(Zeroizing::new(
            s.encode_utf16().chain(std::iter::once(0)).collect(),
        ))
    }

    pub fn pcwstr(&self) -> PCWSTR {
        PCWSTR(self.0.as_ptr())
    }
}

pub fn module_handle() -> HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|m| HINSTANCE(m.0))
        .unwrap_or_default()
}

/// 图标资源编号,与 `app.rc` 保持一致。
const APP_ICON_ID: usize = 1;

/// 程序图标,取自 exe 内嵌的资源(编号 1 的图标组,含 16/32/48/256 多档)。
fn app_icon() -> HICON {
    // MAKEINTRESOURCE:把整数 ID 直接当指针传。
    unsafe { LoadIconW(Some(module_handle()), PCWSTR(APP_ICON_ID as *const u16)) }
        .unwrap_or_default()
}

pub fn register_class(name: &str, proc: WndProc) -> bool {
    let class_name = Wz::new(name);
    let class = WNDCLASSW {
        // CS_HREDRAW | CS_VREDRAW:窗口尺寸一变就**整个**失效重画。
        // 少了它,从最大化还原时只会重画新暴露的小块区域,容易留下残影。
        style: WNDCLASS_STYLES(CS_HREDRAW.0 | CS_VREDRAW.0),
        lpfnWndProc: Some(proc),
        cbClsExtra: 0,
        cbWndExtra: 0,
        hInstance: module_handle(),
        hIcon: app_icon(),
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        // 经典习语:`(HBRUSH)(COLOR_WINDOW + 1)` 表示「系统颜色索引」而不是画刷句柄。
        // 用 GetStockObject(COLOR_WINDOW) 是不对的 —— 那里的 5 号是 NULL_BRUSH(空画刷),
        // 会导致窗口从不擦除背景,新暴露的区域呈现黑块。
        hbrBackground: HBRUSH((COLOR_WINDOW.0 as usize + 1) as *mut c_void),
        lpszMenuName: PCWSTR::null(),
        lpszClassName: class_name.pcwstr(),
    };

    unsafe { RegisterClassW(&class) != 0 }
}

#[allow(clippy::too_many_arguments)]
pub fn create_window(
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    parent: HWND,
    id: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> HWND {
    create_window_with_param(
        class,
        text,
        style,
        ex_style,
        parent,
        id,
        x,
        y,
        w,
        h,
        std::ptr::null(),
    )
}

/// `param` 会作为 `CREATESTRUCTW.lpCreateParams` 传给窗口过程,
/// 便于在 `WM_CREATE` 阶段就把状态指针挂到窗口上。
#[allow(clippy::too_many_arguments)]
pub fn create_window_with_param(
    class: &str,
    text: &str,
    style: u32,
    ex_style: u32,
    parent: HWND,
    id: usize,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    param: *const c_void,
) -> HWND {
    let class_name = Wz::new(class);
    let window_name = Wz::new(text);

    unsafe {
        CreateWindowExW(
            WINDOW_EX_STYLE(ex_style),
            class_name.pcwstr(),
            window_name.pcwstr(),
            WINDOW_STYLE(style),
            x,
            y,
            w,
            h,
            Some(parent),
            Some(HMENU(id as *mut c_void)),
            Some(module_handle()),
            Some(param),
        )
        .unwrap_or_default()
    }
}

/// `WM_CREATE`:取出 lparam 里的状态指针,挂到窗口上。
///
/// 状态由 `dialog::open`(或主窗口的 `run_main_inner`)用 `Box::into_raw` 交出;
/// 调用方随后用 [`state_ref`] 重新取。窗口销毁时由各自的收尾负责回收。
///
/// # Safety
/// 只能在窗口过程的 `WM_CREATE` 分支里调用,且 `lparam` 必须是该消息的原始参数。
pub unsafe fn attach_state<T>(hwnd: HWND, lparam: LPARAM) {
    let ptr = unsafe { create_param(lparam) } as *mut T;
    set_user_data(hwnd, ptr as *mut c_void);
}

/// 在 `WM_CREATE` 中取回 `create_window_with_param` 传入的指针。
///
/// # Safety
/// 只能在窗口过程的 `WM_CREATE` 分支里调用,且 `lparam` 必须是该消息的原始参数。
pub unsafe fn create_param(lparam: LPARAM) -> *mut c_void {
    let cs = unsafe { &*(lparam.0 as *const windows::Win32::UI::WindowsAndMessaging::CREATESTRUCTW) };
    cs.lpCreateParams
}

pub fn set_user_data(hwnd: HWND, ptr: *mut c_void) {
    unsafe {
        SetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA), ptr as isize);
    }
}

/// 取回窗口的自定义状态指针;未设置时返回空指针。
///
/// # Safety
/// 调用方需保证该窗口确实存的是 `T` 的指针。
pub unsafe fn user_data<T>(hwnd: HWND) -> *mut T {
    unsafe { GetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(GWLP_USERDATA)) as *mut T }
}

/// 便捷封装:窗口状态引用。
///
/// # Safety
/// 同 [`user_data`]。
///
/// 返回的 `&mut` 生命周期不受约束,因此**不要把它存进局部变量后跨过可能重入本
/// 窗口过程的调用继续持有**:定时器、模态对话框、`MessageBox`(见 [`confirm`] /
/// [`info`] / [`error`])都会在调用期间派发消息,可能再次 `state_ref` 出同一对象
/// 的 `&mut`,那就是两个可变引用别名。跨过这类调用就"用时重新取"。
pub unsafe fn state_ref<'a, T>(hwnd: HWND) -> &'a mut T {
    unsafe { &mut *user_data::<T>(hwnd) }
}

// ---------- 消息与窗口操作(隐藏新类型) ----------

pub fn send_msg(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> isize {
    unsafe { SendMessageW(hwnd, msg, Some(WPARAM(wparam)), Some(LPARAM(lparam))).0 }
}

pub fn post_message(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> bool {
    unsafe { PostMessageW(Some(hwnd), msg, WPARAM(wparam), LPARAM(lparam)).is_ok() }
}

pub fn show_window(hwnd: HWND, cmd: i32) {
    unsafe {
        let _ = ShowWindow(hwnd, windows::Win32::UI::WindowsAndMessaging::SHOW_WINDOW_CMD(cmd));
    }
}

/// 显示/隐藏控件。
pub fn set_visible(hwnd: HWND, visible: bool) {
    show_window(hwnd, if visible { SW_SHOW } else { SW_HIDE });
}

pub fn enable(hwnd: HWND, enabled: bool) {
    unsafe {
        let _ = EnableWindow(hwnd, enabled);
    }
}

pub fn set_focus(hwnd: HWND) {
    unsafe {
        let _ = SetFocus(Some(hwnd));
    }
}

/// 取窗口客户区大小。
pub fn client_size(hwnd: HWND) -> (i32, i32) {
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rect);
    }
    (rect.right - rect.left, rect.bottom - rect.top)
}

/// 设置编辑框的左右内边距(逻辑像素)。
///
/// 只读字段不画边框(见 dlg_detail),靠这里让文字不贴边。
pub fn set_edit_margins(hwnd: HWND, horizontal: i32) {
    let px = (scale(horizontal) as isize) & 0xFFFF;
    send_msg(hwnd, EM_SETMARGINS, EC_LEFTMARGIN | EC_RIGHTMARGIN, px | (px << 16));
}

/// 切换编辑框的密码字符:`None` 表示明文显示。
pub fn set_password_char(hwnd: HWND, ch: Option<char>) {
    let value = match ch {
        None => 0usize,
        Some(c) => c as usize,
    };
    send_msg(hwnd, EM_SETPASSWORDCHAR, value, 0);
    // 让控件重绘。
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(Some(hwnd), None, true);
    }
}

/// 弹出系统文件对话框。`save` 为真表示「另存为」。
pub fn pick_file(owner: HWND, save: bool, filter: &[(&str, &str)], default_name: &str) -> Option<String> {
    use windows::Win32::UI::Controls::Dialogs::{
        GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT,
        OFN_PATHMUSTEXIST, OPENFILENAMEW, OPEN_FILENAME_FLAGS,
    };

    // 过滤器需要内嵌 NUL,手动拼成宽字符缓冲。
    let mut filter_buf: Vec<u16> = Vec::new();
    for (label, pattern) in filter {
        filter_buf.extend(label.encode_utf16());
        filter_buf.push(0);
        filter_buf.extend(pattern.encode_utf16());
        filter_buf.push(0);
    }
    filter_buf.push(0);

    let mut name: Vec<u16> = vec![0u16; 1024];
    for (i, unit) in default_name.encode_utf16().take(1023).enumerate() {
        name[i] = unit;
    }

    let flags = OFN_PATHMUSTEXIST.0
        | OFN_FILEMUSTEXIST.0
        | if save { OFN_OVERWRITEPROMPT.0 } else { 0 };

    let mut ofn = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter_buf.as_ptr()),
        nFilterIndex: 1,
        lpstrFile: PWSTR(name.as_mut_ptr()),
        nMaxFile: name.len() as u32,
        Flags: OPEN_FILENAME_FLAGS(flags),
        ..Default::default()
    };

    let ok = unsafe {
        if save {
            GetSaveFileNameW(&mut ofn).as_bool()
        } else {
            GetOpenFileNameW(&mut ofn).as_bool()
        }
    };

    if !ok {
        return None;
    }

    let end = name.iter().position(|&c| c == 0).unwrap_or(name.len());
    Some(String::from_utf16_lossy(&name[..end]))
}

/// 用系统默认浏览器打开网址。成功返回 `Ok(())`,否则返回可以直接给用户看的原因。
///
/// 先把网址过一遍可移植核心的允许名单([`crate::url::normalize_http_url`]):
/// 只有 http(s) 会被交给 `ShellExecuteW`。这不是多此一举 —— `ShellExecuteW`
/// 是按协议分派的,`file:` 能直接执行本地程序,所以允许名单必须是白名单。
pub fn open_url_in_browser(raw: &str) -> Result<(), String> {
    use windows::Win32::UI::Shell::ShellExecuteW;

    let url = crate::url::normalize_http_url(raw).ok_or_else(|| {
        "这个网址打不开。\n\n只支持 http/https 链接（没写协议按 https 补全）；本地路径、file:、javascript: 不会交给系统打开。"
            .to_string()
    })?;

    let target = Wz::new(&url);
    let verb = Wz::new("open");
    let result = unsafe {
        ShellExecuteW(
            None,
            verb.pcwstr(),
            target.pcwstr(),
            PCWSTR::null(),
            PCWSTR::null(),
            // 用 windows crate 的类型化常量(sys.rs 里那个是给 ShowWindow 用的 i32)。
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };

    // ShellExecuteW 的返回值 <= 32 表示失败(见 Win32 文档)。
    if result.0 as usize > 32 {
        Ok(())
    } else {
        Err(format!(
            "没能打开浏览器（错误码 {}），请确认默认浏览器可用。",
            result.0 as usize
        ))
    }
}

/// 取窗口位置(屏幕坐标)。
pub fn window_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    let mut rect = windows::Win32::Foundation::RECT::default();
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(hwnd, &mut rect);
    }
    (
        rect.left,
        rect.top,
        rect.right - rect.left,
        rect.bottom - rect.top,
    )
}

/// 窗口所在显示器的工作区(不含任务栏);取不到时返回 `None`。
pub fn work_area(hwnd: HWND) -> Option<(i32, i32, i32, i32)> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };

    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return None;
        }
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        Some((
            info.rcWork.left,
            info.rcWork.top,
            info.rcWork.right,
            info.rcWork.bottom,
        ))
    }
}

pub fn update_window(hwnd: HWND) {
    unsafe {
        let _ = UpdateWindow(hwnd);
    }
}

pub fn destroy_window(hwnd: HWND) {
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

pub fn set_foreground(hwnd: HWND) {
    unsafe {
        let _ = SetForegroundWindow(hwnd);
    }
}

pub fn set_timer(hwnd: HWND, id: usize, ms: u32) {
    unsafe {
        SetTimer(Some(hwnd), id, ms, None);
    }
}

pub fn kill_timer(hwnd: HWND, id: usize) {
    unsafe {
        let _ = KillTimer(Some(hwnd), id);
    }
}

// ---------- 文本 ----------

/// 读控件文本。非敏感内容用它;密码/恢复码请用 [`get_secret`]。
pub fn get_text(hwnd: HWND) -> String {
    get_secret(hwnd).to_string()
}

/// 读取控件文本,丢弃时自动清零 —— 用于登录密码、条目密码、恢复码这类内容。
pub fn get_secret(hwnd: HWND) -> Zeroizing<String> {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return Zeroizing::new(String::new());
    }
    // 中间那个宽字符缓冲里也是明文,同样包起来。
    let mut buf = Zeroizing::new(vec![0u16; len as usize + 1]);
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        return Zeroizing::new(String::new());
    }
    Zeroizing::new(String::from_utf16_lossy(&buf[..n as usize]))
}

pub fn set_text(hwnd: HWND, text: &str) {
    let w = Wz::new(text);
    unsafe {
        let _ = SetWindowTextW(hwnd, w.pcwstr());
    }
}

// ---------- 控件 ----------

pub fn is_checked(hwnd: HWND) -> bool {
    send_msg(hwnd, BM_GETCHECK, 0, 0) != 0
}

pub fn set_checked(hwnd: HWND, checked: bool) {
    send_msg(hwnd, BM_SETCHECK, usize::from(checked), 0);
}

/// 建一个纯色画刷(配套 [`delete_brush`] 回收)。
pub fn create_solid_brush(color: u32) -> HBRUSH {
    unsafe { CreateSolidBrush(COLORREF(color)) }
}

/// 回收画刷。
pub fn delete_brush(brush: HBRUSH) {
    unsafe {
        let _ = DeleteObject(HGDIOBJ(brush.0));
    }
}

/// 往组合框 / 列表框追加一项文本(两种控件的 ADDSTRING 消息形状一致)。
fn add_string(hwnd: HWND, message: u32, text: &str) {
    let w = Wz::new(text);
    unsafe {
        SendMessageW(hwnd, message, None, Some(LPARAM(w.0.as_ptr() as isize)));
    }
}

pub fn listbox_add(hwnd: HWND, text: &str) {
    add_string(hwnd, LB_ADDSTRING, text);
}

pub fn listbox_clear(hwnd: HWND) {
    send_msg(hwnd, LB_RESETCONTENT, 0, 0);
}

pub fn combo_add(hwnd: HWND, text: &str) {
    add_string(hwnd, CB_ADDSTRING, text);
}

pub fn combo_set_index(hwnd: HWND, index: i32) {
    send_msg(hwnd, CB_SETCURSEL, index.max(0) as usize, 0);
}

pub fn combo_index(hwnd: HWND) -> i32 {
    send_msg(hwnd, CB_GETCURSEL, 0, 0) as i32
}

/// 读取组合框 / 列表框某项的文本:先问长度开缓冲,再取内容。
fn item_text(hwnd: HWND, length_message: u32, text_message: u32, index: i32) -> String {
    if index < 0 {
        return String::new();
    }
    let len = send_msg(hwnd, length_message, index as usize, 0);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = send_msg(hwnd, text_message, index as usize, buf.as_mut_ptr() as isize);
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

pub fn combo_item_text(hwnd: HWND, index: i32) -> String {
    item_text(hwnd, CB_GETLBTEXTLEN, CB_GETLBTEXT, index)
}

// ---------- 列表 / 列表视图 ----------

pub fn listbox_index(hwnd: HWND) -> i32 {
    send_msg(hwnd, LB_GETCURSEL, 0, 0) as i32
}

pub fn listbox_text(hwnd: HWND, index: i32) -> String {
    item_text(hwnd, LB_GETTEXTLEN, LB_GETTEXT, index)
}

/// 按文本精确查找列表框项,找不到返回 -1。
pub fn listbox_find(hwnd: HWND, text: &str) -> i32 {
    let w = Wz::new(text);
    send_msg(hwnd, LB_FINDSTRINGEXACT, usize::MAX, w.0.as_ptr() as isize) as i32
}

/// 设置列表框的横向滚动范围(文字比控件宽时出现横向滚动条)。
pub fn listbox_set_horizontal_extent(hwnd: HWND, width: i32) {
    send_msg(hwnd, LB_SETHORIZONTALEXTENT, width.max(0) as usize, 0);
}

/// 设置虚拟列表(`LVS_OWNERDATA`)的条目总数。
pub fn listview_set_item_count(hwnd: HWND, count: usize) {
    send_msg(hwnd, LVM_SETITEMCOUNT, count, 0);
}

/// 让列表视图整块重画(不擦背景,避免闪烁)。
pub fn listview_refresh(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, false);
    }
}

pub fn listview_add_column(hwnd: HWND, index: i32, width: i32, title: &str) {
    let text = Wz::new(title);
    let mut column = LVCOLUMNW {
        mask: LVCOLUMNW_MASK(LVCF_FMT.0 | LVCF_WIDTH.0 | LVCF_TEXT.0 | LVCF_SUBITEM.0),
        fmt: LVCOLUMNW_FORMAT(LVCFMT_LEFT.0),
        cx: width,
        pszText: PWSTR(text.0.as_ptr() as *mut u16),
        cchTextMax: 0,
        iSubItem: index,
        iImage: 0,
        iOrder: 0,
        cxMin: 0,
        cxDefault: 0,
        cxIdeal: 0,
    };
    send_msg(
        hwnd,
        LVM_INSERTCOLUMNW,
        index as usize,
        &mut column as *mut _ as isize,
    );
}

pub fn listview_set_extended_style(hwnd: HWND, style: u32) {
    send_msg(hwnd, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, style as isize);
}

/// 设置某一列的宽度(LVM_SETCOLUMNWIDTH)。
pub fn listview_set_column_width(hwnd: HWND, column: i32, width: i32) {
    send_msg(hwnd, LVM_SETCOLUMNWIDTH, column as usize, width as isize);
}

/// 获取某一列的宽度(LVM_GETCOLUMNWIDTH)。
pub fn listview_get_column_width(hwnd: HWND, column: i32) -> i32 {
    send_msg(hwnd, LVM_GETCOLUMNWIDTH, column as usize, 0) as i32
}

/// 获取列表视图的表头控件句柄(LVM_GETHEADER)。
pub fn listview_get_header(hwnd: HWND) -> HWND {
    HWND(send_msg(hwnd, LVM_GETHEADER, 0, 0) as *mut std::ffi::c_void)
}

/// 设置列表视图的行高(通过虚拟 ImageList 设置)。
pub fn listview_set_row_height(hwnd: HWND, height: i32) {
    unsafe {
        let himl = windows::Win32::UI::Controls::ImageList_Create(
            1,
            height.max(1),
            windows::Win32::UI::Controls::ILC_COLOR,
            1,
            1,
        );
        if !himl.is_invalid() {
            let prev = send_msg(hwnd, LVM_SETIMAGELIST, LVSIL_SMALL, himl.0);
            if prev != 0 {
                let _ = windows::Win32::UI::Controls::ImageList_Destroy(Some(
                    windows::Win32::UI::Controls::HIMAGELIST(prev),
                ));
            }
        }
    }
}

pub fn listview_selected_index(hwnd: HWND) -> i32 {
    send_msg(hwnd, LVM_GETNEXTITEM, usize::MAX, LVNI_SELECTED as isize) as i32
}

pub fn listview_select(hwnd: HWND, index: i32) {
    if index < 0 {
        return;
    }
    let state = LVIS_SELECTED.0 | LVIS_FOCUSED.0;
    let mut item = LVITEMW {
        mask: LIST_VIEW_ITEM_FLAGS(0),
        iItem: index,
        iSubItem: 0,
        state: LIST_VIEW_ITEM_STATE_FLAGS(state),
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(state),
        pszText: PWSTR::null(),
        cchTextMax: 0,
        iImage: 0,
        lParam: LPARAM(0),
        iIndent: 0,
        iGroupId: 0,
        cColumns: 0,
        puColumns: std::ptr::null_mut(),
        piColFmt: std::ptr::null_mut(),
        iGroup: 0,
    };
    send_msg(hwnd, LVM_SETITEMSTATE, index as usize, &mut item as *mut _ as isize);
}

/// 清除列表视图的全部选中与焦点(LVM_SETITEMSTATE 的 wparam=-1 表示「所有项」)。
///
/// 虚拟列表会按索引保留控件的选中状态 —— 数据重建后原条目可能已不在原位,
/// 不显式清除就会把高亮留在别的行上。
pub fn listview_clear_selection(hwnd: HWND) {
    let mut item = LVITEMW {
        mask: LIST_VIEW_ITEM_FLAGS(0),
        iItem: -1,
        iSubItem: 0,
        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(LVIS_SELECTED.0 | LVIS_FOCUSED.0),
        pszText: PWSTR::null(),
        cchTextMax: 0,
        iImage: 0,
        lParam: LPARAM(0),
        iIndent: 0,
        iGroupId: 0,
        cColumns: 0,
        puColumns: std::ptr::null_mut(),
        piColFmt: std::ptr::null_mut(),
        iGroup: 0,
    };
    send_msg(hwnd, LVM_SETITEMSTATE, usize::MAX, &mut item as *mut _ as isize);
}

// ---------- 消息框 ----------

pub fn msg_box(owner: HWND, text: &str, title: &str, flags: u32) -> i32 {
    let t = Wz::new(text);
    let c = Wz::new(title);
    unsafe { MessageBoxW(Some(owner), t.pcwstr(), c.pcwstr(), MESSAGEBOX_STYLE(flags)).0 }
}

pub fn confirm(owner: HWND, text: &str, title: &str) -> bool {
    msg_box(owner, text, title, MB_YESNO | MB_ICONQUESTION | MB_DEFBUTTON2) == IDYES
}

pub fn warn(owner: HWND, text: &str, title: &str) {
    msg_box(owner, text, title, MB_OK | MB_ICONWARNING);
}

pub fn error(owner: HWND, text: &str, title: &str) {
    msg_box(owner, text, title, MB_OK | MB_ICONERROR);
}

pub fn info(owner: HWND, text: &str, title: &str) {
    msg_box(owner, text, title, MB_OK | MB_ICONINFORMATION);
}

// ---------- 字体 ----------

pub fn create_ui_font(bold: bool, dpi: u32) -> HFONT {
    let _ = dpi;
    unsafe {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };

        let ok = SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some(&mut metrics as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );

        let mut lf: LOGFONTW = if ok.is_ok() {
            metrics.lfMessageFont
        } else {
            LOGFONTW::default()
        };

        // `lfMessageFont` 的高度**已经按系统 DPI 缩放好了**
        // (168 DPI 下 9pt 就是 -21),所以不能再乘 dpi/96,那会放大两次。
        // 但系统默认 9pt 在 2560×1600 这类高密度屏上偏小,整体放大一档提升可读性。
        let base_height = if lf.lfHeight < 0 { lf.lfHeight } else { -12 };
        lf.lfHeight = ((base_height as f32) * FONT_SCALE).round() as i32;

        if bold {
            lf.lfWeight = 600; // FW_SEMIBOLD
        }
        lf.lfQuality = CLEARTYPE_QUALITY;

        CreateFontIndirectW(&lf)
    }
}

/// 字号相对系统默认的放大倍数(1.0 = 系统标准 9pt)。
const FONT_SCALE: f32 = 1.3;

/// 供自检输出:系统消息字体的原始高度与实际使用的高度。
pub fn message_font_height() -> (i32, i32) {
    unsafe {
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let ok = SystemParametersInfoW(
            windows::Win32::UI::WindowsAndMessaging::SPI_GETNONCLIENTMETRICS,
            metrics.cbSize,
            Some(&mut metrics as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if ok.is_err() {
            return (0, 0);
        }
        let raw = metrics.lfMessageFont.lfHeight;
        let used = ((raw as f32) * FONT_SCALE).round() as i32;
        (raw, used)
    }
}

/// 按当前界面 DPI 缩放逻辑像素(对话框布局用)。
pub fn scale(v: i32) -> i32 {
    let dpi = if super::app::is_ready() {
        super::app::state().dpi
    } else {
        system_dpi()
    }
    .max(96);
    (v as f32 * dpi as f32 / 96.0).round() as i32
}

/// 将当前界面 DPI 下的物理像素换算回 96 DPI 基准的逻辑像素。
pub fn unscale(v: i32) -> i32 {
    let dpi = if super::app::is_ready() {
        super::app::state().dpi
    } else {
        system_dpi()
    }
    .max(96);
    ((v as f32 * 96.0) / dpi as f32).round() as i32
}

/// 取系统 DPI。
///
/// **必须在创建窗口之前调用**:`WM_CREATE` 阶段窗口还没关联到显示器,
/// `GetDpiForWindow` 会返回 0,导致整个界面按 96 DPI 排版。
pub fn system_dpi() -> u32 {
    let dpi = unsafe { GetDpiForSystem() };
    if dpi == 0 { 96 } else { dpi }
}

/// 静态文本的绘制:背景透明、指定文字颜色,返回应使用的画刷。
///
/// 默认情况下静态控件会用 BTNFACE(浅灰)刷背景,在白底窗口里会出现灰色条。
pub fn paint_static_label(hdc: HDC, text_color: u32) -> isize {
    unsafe {
        SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT
        SetTextColor(hdc, COLORREF(text_color));
        GetSysColorBrush(COLOR_WINDOW).0 as isize
    }
}

/// `WM_CTLCOLORSTATIC` 的统一应答(白底窗口):透明文字 + 正文色,返回窗口底色画刷。
///
/// 不处理的话,静态文本会按 STATIC 类的默认画面画出一块灰底矩形 ——
/// 在白色对话框上就是每个标签后面拖一条灰。
pub fn static_label_reply(hdc_raw: usize) -> isize {
    paint_static_label(HDC(hdc_raw as *mut c_void), TEXT)
}

/// 只读值字段的 `WM_CTLCOLORSTATIC` 应答:浅灰底 + 正文色,由控件用返回的画刷填充。
///
/// 文字保持正文色而不是灰掉 —— 字段仍可点击复制,灰色会被读成「不可用」。
pub fn readonly_field_reply(hdc_raw: usize, brush: HBRUSH) -> isize {
    let hdc = HDC(hdc_raw as *mut c_void);
    unsafe {
        SetBkColor(hdc, COLORREF(READONLY_BG));
        SetTextColor(hdc, COLORREF(TEXT));
    }
    brush.0 as isize
}

pub fn delete_font(font: HFONT) {
    if !font.is_invalid() {
        unsafe {
            let _ = DeleteObject(HGDIOBJ(font.0));
        }
    }
}

/// 强制立即重画整个窗口(含所有子控件)。
///
/// 只调 `InvalidateRect` 是不够的:它只是"标记"待重画,而且默认不含子窗口。
/// 从最大化还原时,窗口变小时新暴露出来的区域必须马上擦干净,否则会残留旧画面。
pub fn redraw_all(hwnd: HWND) {
    unsafe {
        let _ = RedrawWindow(
            Some(hwnd),
            None,
            None,
            RDW_INVALIDATE | RDW_ERASE | RDW_ALLCHILDREN | RDW_UPDATENOW,
        );
    }
}

// ---------- 弹出菜单 ----------

/// 右键弹出菜单。菜单项 id 由调用方给定,`track` 返回用户选择的那一项。
pub struct PopupMenu {
    handle: HMENU,
    /// 作为子菜单挂到父菜单上时由父菜单负责销毁,自己不能再 DestroyMenu。
    owned: bool,
}

impl PopupMenu {
    pub fn new() -> Self {
        Self {
            handle: unsafe { CreatePopupMenu() }.unwrap_or_default(),
            owned: true,
        }
    }

    /// 新建一个子菜单并挂到当前菜单上;返回的子菜单不可再单独销毁。
    pub fn submenu(&mut self, text: &str) -> PopupMenu {
        let inner = unsafe { CreatePopupMenu() }.unwrap_or_default();
        let w = Wz::new(text);
        unsafe {
            let _ = AppendMenuW(self.handle, MF_POPUP, inner.0 as usize, w.pcwstr());
        }
        PopupMenu {
            handle: inner,
            owned: false,
        }
    }

    pub fn add(&mut self, id: usize, text: &str) {
        let w = Wz::new(text);
        unsafe {
            let _ = AppendMenuW(self.handle, MF_STRING, id, w.pcwstr());
        }
    }

    /// 添加一项;`enabled` 为假时显示为灰色且不可点击。
    pub fn add_item(&mut self, id: usize, text: &str, enabled: bool) {
        let w = Wz::new(text);
        let flags = if enabled { MF_STRING } else { MF_GRAYED };
        unsafe {
            let _ = AppendMenuW(self.handle, flags, id, w.pcwstr());
        }
    }

    pub fn add_separator(&mut self) {
        unsafe {
            let _ = AppendMenuW(self.handle, MF_SEPARATOR, 0, PCWSTR::null());
        }
    }

    /// 在鼠标处弹出;返回用户选择的 id(取消则为 None)。
    pub fn track(&self, hwnd: HWND) -> Option<usize> {
        let mut pt = POINT::default();
        unsafe {
            if GetCursorPos(&mut pt).is_err() {
                return None;
            }
            // 没有先设前台的话,菜单可能不消失。
            let _ = SetForegroundWindow(hwnd);
            let chosen = TrackPopupMenu(
                self.handle,
                TPM_RETURNCMD | TPM_RIGHTBUTTON,
                pt.x,
                pt.y,
                None, // nreserved
                hwnd,
                None,
            );
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
            if chosen.0 == 0 {
                None
            } else {
                Some(chosen.0 as usize)
            }
        }
    }
}

impl Default for PopupMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for PopupMenu {
    fn drop(&mut self) {
        if self.owned && !self.handle.is_invalid() {
            unsafe {
                let _ = DestroyMenu(self.handle);
            }
        }
    }
}

/// 屏幕坐标是否落在某个窗口的矩形范围内。
pub fn point_in_window(hwnd: HWND, pt: POINT) -> bool {
    let (x, y, w, h) = window_rect(hwnd);
    pt.x >= x && pt.x < x + w && pt.y >= y && pt.y < y + h
}

/// 鼠标当前所在的屏幕坐标。
pub fn cursor_pos() -> POINT {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt
}

/// 屏幕坐标 → 指定窗口的客户区坐标。
pub fn screen_to_client(hwnd: HWND, mut pt: POINT) -> POINT {
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    pt
}

/// `LVHITTESTINFO` 的等价结构(自己声明,避免依赖 crate 的类型名)。
#[repr(C)]
struct LvHitTestInfo {
    pt: POINT,
    flags: u32,
    i_item: i32,
    i_sub_item: i32,
    /// Vista+ 的 `LVHITTESTINFO` 还有 `iGroup`。未启用分组时恒为 0,但结构体
    /// 必须留出这块内存 —— 否则将来启用分组时控件会写到结构体外面。
    i_group: i32,
}

// ---------- 分类标签条(自绘,Excel 风格)----------
//
// 不用 SysTabControl32:它的颜色由主题写死,做不出「选中 = 白色卡片 + 彩色下划线」
// 的样式,而且它的「页面区」会和压在它上面的列表控件打架。
// 这里是一个普通的子窗口,自己绘制、自己命中测试。

/// 用给定字体测量一段文字的像素宽度。
pub fn text_width(hwnd: HWND, font: HFONT, text: &str) -> i32 {
    use windows::Win32::Graphics::Gdi::GetTextExtentPoint32W;

    let wide: Vec<u16> = text.encode_utf16().collect();
    unsafe {
        let dc = windows::Win32::Graphics::Gdi::GetDC(Some(hwnd));
        if dc.is_invalid() {
            return 0;
        }
        let old = if font.is_invalid() {
            Default::default()
        } else {
            windows::Win32::Graphics::Gdi::SelectObject(dc, HGDIOBJ(font.0))
        };
        let mut size = windows::Win32::Foundation::SIZE::default();
        let ok = GetTextExtentPoint32W(dc, &wide, &mut size);
        if !old.is_invalid() {
            windows::Win32::Graphics::Gdi::SelectObject(dc, old);
        }
        let _ = windows::Win32::Graphics::Gdi::ReleaseDC(Some(hwnd), dc);
        if ok.as_bool() { size.cx } else { 0 }
    }
}

/// 取已加载模块的路径(用于判断 comctl32 是 v5 还是 v6)。
pub fn module_path(name: &str) -> Option<String> {
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};

    let wide = Wz::new(name);
    let module = unsafe { GetModuleHandleW(wide.pcwstr()) }.ok()?;
    let mut buf = vec![0u16; 512];
    let len = unsafe { GetModuleFileNameW(Some(module), &mut buf) };
    Some(String::from_utf16_lossy(&buf[..len as usize]))
}

struct TabStrip {
    names: Vec<String>,
    selected: i32,
    font: HFONT,
}

// 界面只有一个标签条、且只在 GUI 线程访问;HFONT 只是个句柄。
unsafe impl Send for TabStrip {}

fn tab_strip() -> &'static Mutex<TabStrip> {
    static STRIP: OnceLock<Mutex<TabStrip>> = OnceLock::new();
    STRIP.get_or_init(|| {
        Mutex::new(TabStrip {
            names: Vec::new(),
            selected: 0,
            font: HFONT::default(),
        })
    })
}

// COLORREF 是 BGR 序;颜色统一收在 `super::tokens`。

/// 标签条上的尺寸(已按 DPI 缩放):(左右内边距, 最小宽度, 下划线高度)
fn strip_metrics(hwnd: HWND) -> (i32, i32, i32) {
    let dpi = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32;
    let s = |v: i32| (v as f32 * dpi / 96.0).round() as i32;
    (s(18), s(72), s(3))
}

/// 每个标签的 [left, right)。不缓存,总是现算,免得和外框尺寸不同步。
fn strip_layout(hwnd: HWND, names: &[String], font: HFONT) -> Vec<(i32, i32)> {
    let (pad, min_w, _) = strip_metrics(hwnd);
    let mut out = Vec::with_capacity(names.len());
    let mut x = 0;
    for name in names {
        let text_w = text_width(hwnd, font, name);
        let width = (text_w + pad * 2).max(min_w);
        out.push((x, x + width));
        x += width;
    }
    out
}

unsafe fn paint_strip(hwnd: HWND) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{
        BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, SelectObject,
        SetBkMode, SetTextColor, BACKGROUND_MODE, DRAW_TEXT_FORMAT, PAINTSTRUCT,
    };

    let mut ps = PAINTSTRUCT::default();
    let hdc = unsafe { BeginPaint(hwnd, &mut ps) };

    let (client_w, client_h) = client_size(hwnd);
    let (_, _, accent_h) = strip_metrics(hwnd);

    let (names, selected, font) = {
        let strip = tab_strip().lock().unwrap();
        (strip.names.clone(), strip.selected, strip.font)
    };
    let layout = strip_layout(hwnd, &names, font);

    unsafe {
        let full = RECT {
            left: 0,
            top: 0,
            right: client_w,
            bottom: client_h,
        };
        let background = CreateSolidBrush(COLORREF(STRIP_BG));
        FillRect(hdc, &full, background);
        let _ = DeleteObject(HGDIOBJ(background.0));

        let old_font = if font.is_invalid() {
            Default::default()
        } else {
            SelectObject(hdc, HGDIOBJ(font.0))
        };
        SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT

        for (index, name) in names.iter().enumerate() {
            let (left, right) = layout[index];
            let chosen = index as i32 == selected;
            let mut rect = RECT {
                left,
                top: 0,
                right,
                bottom: client_h,
            };

            if chosen {
                let card = CreateSolidBrush(COLORREF(STRIP_CARD_BG));
                FillRect(hdc, &rect, card);
                let _ = DeleteObject(HGDIOBJ(card.0));

                let accent = CreateSolidBrush(COLORREF(STRIP_ACCENT));
                let underline = RECT {
                    left,
                    top: (client_h - accent_h).max(0),
                    right,
                    bottom: client_h,
                };
                FillRect(hdc, &underline, accent);
                let _ = DeleteObject(HGDIOBJ(accent.0));
            } else {
                let separator = CreateSolidBrush(COLORREF(STRIP_SEPARATOR));
                let inset = (client_h / 5).max(2);
                let line = RECT {
                    left: right - 1,
                    top: inset,
                    right,
                    bottom: client_h - inset,
                };
                FillRect(hdc, &line, separator);
                let _ = DeleteObject(HGDIOBJ(separator.0));
            }

            let mut wide: Vec<u16> = name.encode_utf16().collect();
            SetTextColor(
                hdc,
                COLORREF(if chosen { STRIP_TEXT_ON } else { STRIP_TEXT }),
            );
            DrawTextW(
                hdc,
                &mut wide,
                &mut rect,
                DRAW_TEXT_FORMAT(DT_CENTER | DT_VCENTER | DT_SINGLELINE),
            );
        }

        if !old_font.is_invalid() {
            SelectObject(hdc, old_font);
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn strip_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_SETFONT => {
            tab_strip().lock().unwrap().font = HFONT(wparam.0 as *mut core::ffi::c_void);
            invalidate(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1), // 背景全部由 WM_PAINT 负责,避免闪烁
        WM_PAINT => {
            unsafe { paint_strip(hwnd) };
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let x = (lparam.0 & 0xFFFF) as u16 as i32;
            let index = {
                let (names, font) = {
                    let strip = tab_strip().lock().unwrap();
                    (strip.names.clone(), strip.font)
                };
                let layout = strip_layout(hwnd, &names, font);
                layout
                    .iter()
                    .position(|(left, right)| x >= *left && x < *right)
                    .map(|i| i as i32)
                    .unwrap_or(-1)
            };

            if index >= 0 {
                let changed = {
                    let mut strip = tab_strip().lock().unwrap();
                    let changed = strip.selected != index;
                    strip.selected = index;
                    changed
                };
                if changed {
                    // 通知父窗口重新筛选。注意此时不持有锁。
                    let parent = unsafe { GetParent(hwnd) }.unwrap_or_default();
                    unsafe {
                        SendMessageW(
                            parent,
                            TSM_TAB_CHANGED,
                            Some(WPARAM(index as usize)),
                            Some(LPARAM(hwnd.0 as isize)),
                        );
                    }
                }
                invalidate(hwnd);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// 注册标签条窗口类(重复注册会被忽略)。
pub fn register_tab_strip_class() {
    let _ = register_class("PnbTabStrip", strip_proc);
}

pub fn tabs_clear(hwnd: HWND) {
    {
        let mut strip = tab_strip().lock().unwrap();
        strip.names.clear();
        strip.selected = 0;
    }
    invalidate(hwnd);
}

pub fn tabs_add(hwnd: HWND, text: &str) -> i32 {
    let index = {
        let mut strip = tab_strip().lock().unwrap();
        strip.names.push(text.to_string());
        strip.names.len() as i32 - 1
    };
    invalidate(hwnd);
    index
}

pub fn tabs_index(_hwnd: HWND) -> i32 {
    tab_strip().lock().unwrap().selected
}

pub fn tabs_set_index(hwnd: HWND, index: i32) {
    let changed = {
        let mut strip = tab_strip().lock().unwrap();
        let changed = strip.selected != index;
        strip.selected = index;
        changed
    };
    if changed {
        invalidate(hwnd);
    }
}

/// 所有标签的名字(测试用)。
pub fn tabs_names() -> Vec<String> {
    tab_strip().lock().unwrap().names.clone()
}

pub fn tabs_hit_test(hwnd: HWND, screen_pt: POINT) -> i32 {
    let local = screen_to_client(hwnd, screen_pt);
    let (names, font) = {
        let strip = tab_strip().lock().unwrap();
        (strip.names.clone(), strip.font)
    };
    strip_layout(hwnd, &names, font)
        .iter()
        .position(|(left, right)| local.x >= *left && local.x < *right)
        .map(|i| i as i32)
        .unwrap_or(-1)
}

/// 命中测试:返回屏幕坐标 `screen_pt` 落在 ListView 的哪一行(负数表示没有)。
pub fn listview_item_at(list: HWND, screen_pt: POINT) -> i32 {
    let local = screen_to_client(list, screen_pt);
    let mut info = LvHitTestInfo {
        pt: local,
        flags: 0,
        i_item: -1,
        i_sub_item: 0,
        i_group: 0,
    };
    send_msg(list, LVM_HITTEST, 0, &mut info as *mut _ as isize) as i32
}

/// 让整个窗口(含子控件)重绘一次(不立即执行)。
pub fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// 移动控件到指定位置。
pub fn move_to(hwnd: HWND, x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::MoveWindow(
            hwnd,
            x,
            y,
            w.max(1),
            h.max(1),
            true,
        );
    }
}

/// 创建带布局的子控件:矩形按 96-DPI 逻辑像素传入,内部统一缩放。
///
/// 各对话框原先各自复制了一份同样的实现,统一收到这里。
pub fn ctl(
    class: &str,
    text: &str,
    style: u32,
    ex: u32,
    parent: HWND,
    id: usize,
    r: (i32, i32, i32, i32),
) -> HWND {
    let handle = create_window(class, text, WS_CHILD | WS_VISIBLE | style, ex, parent, id, 0, 0, 10, 10);
    move_to(handle, scale(r.0), scale(r.1), scale(r.2), scale(r.3));
    handle
}

/// 静态文本(左对齐)。
pub fn label(parent: HWND, text: &str, id: usize, r: (i32, i32, i32, i32)) -> HWND {
    ctl("STATIC", text, SS_LEFT, 0, parent, id, r)
}

/// 给一组控件套用字体。
pub fn apply_font_to(parent: HWND, ids: &[usize], font: HFONT) {
    if font.is_invalid() {
        return;
    }
    for id in ids {
        if let Ok(control) = unsafe { GetDlgItem(Some(parent), *id as i32) } {
            send_msg(control, WM_SETFONT, font.0 as usize, 1);
        }
    }
}

/// 给「动作类」控件套粗体:推送按钮与自绘分类页签。
///
/// 用枚举子窗口而不是逐对话框列 ID:按钮散落在七个对话框里,手工列表迟早会漏。
pub fn apply_bold_actions(parent: HWND, bold: HFONT) {
    if bold.is_invalid() {
        return;
    }
    unsafe {
        let _ = EnumChildWindows(Some(parent), Some(bold_action_proc), LPARAM(bold.0 as isize));
    }
}

unsafe extern "system" fn bold_action_proc(child: HWND, lparam: LPARAM) -> BOOL {
    let bold = HFONT(lparam.0 as *mut c_void);
    let mut buf = [0u16; 32];
    let len = unsafe { GetClassNameW(child, &mut buf) }.max(0) as usize;
    let class = String::from_utf16_lossy(&buf[..len]);
    // 只认推送按钮;复选框、单选、分组框保持正文字重。
    let push_button = class == "Button" && {
        let style = unsafe { GetWindowLongPtrW(child, WINDOW_LONG_PTR_INDEX(GWL_STYLE)) } as u32;
        matches!(style & BS_TYPEMASK, BS_PUSHBUTTON | BS_DEFPUSHBUTTON)
    };
    if push_button || class == "PnbTabStrip" {
        send_msg(child, WM_SETFONT, bold.0 as usize, 1);
    }
    BOOL(1)
}

/// 列头用粗体:表头是列表的「标题」,与正文拉开层级。
pub fn listview_bold_header(list: HWND, bold: HFONT) {
    if bold.is_invalid() {
        return;
    }
    let header = HWND(send_msg(list, LVM_GETHEADER, 0, 0) as *mut c_void);
    if !header.is_invalid() {
        send_msg(header, WM_SETFONT, bold.0 as usize, 1);
    }
}

/// 列头的列数(HDM_GETITEMCOUNT)。
pub fn header_item_count(header: HWND) -> i32 {
    send_msg(header, HDM_GETITEMCOUNT, 0, 0) as i32
}

/// 列头某一列的矩形(HDM_GETITEMRECT,列头客户区坐标)。
pub fn header_item_rect(header: HWND, index: i32) -> Option<windows::Win32::Foundation::RECT> {
    let mut rect = windows::Win32::Foundation::RECT::default();
    let ok = send_msg(header, HDM_GETITEMRECT, index as usize, &mut rect as *mut _ as isize) != 0;
    ok.then_some(rect)
}

/// 子类化列头,交给 `proc` 整块自绘。
///
/// 列头的 `NM_CUSTOMDRAW` 不会被列表视图转发到父窗口(实测),主题画面就只能
/// 从 `WM_PAINT` 这一层整个接管。
pub fn subclass_header(header: HWND, proc: SubclassProc) -> bool {
    unsafe { SetWindowSubclass(header, Some(proc), 1, 0).as_bool() }
}

/// 列头某一列的信息(自绘列头用)。
struct HeaderItem {
    text: String,
    center: bool,
    right: bool,
}

/// 读取列头某一列的文本与对齐方式。
///
/// 自己声明 `HDITEMW`(与 commctrl.h 的布局一致,`LvHitTestInfo` 同理):
/// 自绘只需要其中几个字段,免得与 crate 的 newtype 常量纠缠。
fn header_item(header: HWND, index: i32) -> HeaderItem {
    const HDI_TEXT: u32 = 0x0002;
    const HDI_FORMAT: u32 = 0x0004;
    const HDF_RIGHT: u32 = 0x0001;
    const HDF_CENTER: u32 = 0x0002;

    #[repr(C)]
    struct HdItemW {
        mask: u32,
        cxy: i32,
        psz_text: *mut u16,
        hbm: *mut c_void,
        cch_text_max: i32,
        fmt: u32,
        lparam: isize,
        i_image: i32,
        i_order: i32,
        type_: u32,
        pv_filter: *mut c_void,
        state: u32,
    }

    let mut buf = [0u16; 128];
    let mut item = HdItemW {
        mask: HDI_TEXT | HDI_FORMAT,
        cxy: 0,
        psz_text: buf.as_mut_ptr(),
        hbm: std::ptr::null_mut(),
        cch_text_max: buf.len() as i32,
        fmt: 0,
        lparam: 0,
        i_image: 0,
        i_order: 0,
        type_: 0,
        pv_filter: std::ptr::null_mut(),
        state: 0,
    };
    let ok = send_msg(header, HDM_GETITEMW, index as usize, &mut item as *mut _ as isize) != 0;
    let text = if ok {
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    } else {
        String::new()
    };
    HeaderItem {
        text,
        center: item.fmt & HDF_CENTER != 0,
        right: item.fmt & HDF_RIGHT != 0,
    }
}

/// 自绘一个列头项:灰底、右侧分隔线,再用列头当前字体画出标题。
///
/// 由 `main_ui::paint_header`(列头子类化的 `WM_PAINT`)逐列调用 ——
/// 列头的主题画面整体由那条路径接管(列头的 NM_CUSTOMDRAW 不会转发到父窗口)。
pub fn draw_header_item(
    header: HWND,
    hdc: HDC,
    rect: &windows::Win32::Foundation::RECT,
    index: i32,
    back: HBRUSH,
    separator: HBRUSH,
) {
    use windows::Win32::Foundation::RECT;
    use windows::Win32::Graphics::Gdi::{DrawTextW, FillRect, SelectObject, DRAW_TEXT_FORMAT};

    let item = header_item(header, index);

    unsafe {
        FillRect(hdc, rect, back);
        let line = RECT {
            left: rect.right - 1,
            top: rect.top,
            right: rect.right,
            bottom: rect.bottom,
        };
        FillRect(hdc, &line, separator);
    }

    let font = HFONT(send_msg(header, WM_GETFONT, 0, 0) as *mut c_void);
    let old_font = if font.is_invalid() {
        HGDIOBJ::default()
    } else {
        unsafe { SelectObject(hdc, HGDIOBJ(font.0)) }
    };

    let align = if item.center {
        DT_CENTER
    } else if item.right {
        DT_RIGHT
    } else {
        DT_LEFT
    };
    let mut text_rect = RECT {
        left: rect.left + scale(8),
        top: rect.top,
        right: (rect.right - scale(6)).max(rect.left + scale(8)),
        bottom: rect.bottom,
    };
    let mut wide: Vec<u16> = item.text.encode_utf16().collect();

    unsafe {
        SetBkMode(hdc, BACKGROUND_MODE(1)); // TRANSPARENT
        SetTextColor(hdc, COLORREF(TEXT));
        DrawTextW(
            hdc,
            &mut wide,
            &mut text_rect,
            DRAW_TEXT_FORMAT(DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | align),
        );
        if !old_font.is_invalid() {
            SelectObject(hdc, old_font);
        }
    }
}

