//! Win32 控件与窗口的薄封装。
//!
//! 这里把 `windows` crate 的新类型(各种 `XXX_STYLE`、`SHOW_WINDOW_CMD` 等)
//! 全部消化掉,调用方只需要普通的 u32 / i32,以及 [`sys`](super::sys) 里的常量。

use std::ffi::c_void;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontIndirectW, DeleteObject, GetSysColorBrush, InvalidateRect, RedrawWindow, ScreenToClient,
    SetBkMode, SetTextColor, UpdateWindow, BACKGROUND_MODE, CLEARTYPE_QUALITY, COLOR_WINDOW, HBRUSH,
    HDC, HFONT, HGDIOBJ, LOGFONTW, RDW_ALLCHILDREN, RDW_ERASE, RDW_INVALIDATE, RDW_UPDATENOW,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    LIST_VIEW_ITEM_FLAGS, LIST_VIEW_ITEM_STATE_FLAGS, LVCFMT_LEFT, LVCOLUMNW, LVCOLUMNW_FORMAT,
    LVCOLUMNW_MASK, LVCF_FMT, LVCF_SUBITEM, LVCF_TEXT, LVCF_WIDTH, LVIF_TEXT, LVIS_FOCUSED,
    LVIS_SELECTED, LVITEMW,
};
use windows::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DestroyMenu, DestroyWindow, GetCursorPos,
    GetDlgItem, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HMENU, IDC_ARROW,
    IsZoomed, KillTimer, LoadCursorW, MESSAGEBOX_STYLE, MessageBoxW, MF_GRAYED, MF_SEPARATOR,
    MF_STRING,
    NONCLIENTMETRICSW, PostMessageW, RegisterClassW, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
    SendMessageW, SetForegroundWindow, SetTimer, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
    SystemParametersInfoW, TrackPopupMenu, CS_HREDRAW, CS_VREDRAW, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX, WINDOW_STYLE, WM_NULL, WNDCLASSW, WNDCLASS_STYLES,
};

use super::sys::*;

pub type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;

/// UTF-16 缓冲区,保证在 CreateWindowExW / RegisterClassW 调用期间指针有效。
pub struct Wz(Vec<u16>);

impl Wz {
    pub fn new(s: &str) -> Self {
        Self(s.encode_utf16().chain(std::iter::once(0)).collect())
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
        hIcon: Default::default(),
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

pub fn get_text(hwnd: HWND) -> String {
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
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

pub fn listbox_add(hwnd: HWND, text: &str) {
    let w = Wz::new(text);
    unsafe {
        SendMessageW(hwnd, LB_ADDSTRING, None, Some(LPARAM(w.0.as_ptr() as isize)));
    }
}

pub fn listbox_clear(hwnd: HWND) {
    send_msg(hwnd, LB_RESETCONTENT, 0, 0);
}

pub fn combo_add(hwnd: HWND, text: &str) {
    let w = Wz::new(text);
    unsafe {
        SendMessageW(hwnd, CB_ADDSTRING, None, Some(LPARAM(w.0.as_ptr() as isize)));
    }
}

pub fn combo_clear(hwnd: HWND) {
    send_msg(hwnd, CB_RESETCONTENT, 0, 0);
}

pub fn combo_set_index(hwnd: HWND, index: i32) {
    send_msg(hwnd, CB_SETCURSEL, index.max(0) as usize, 0);
}

pub fn combo_index(hwnd: HWND) -> i32 {
    send_msg(hwnd, CB_GETCURSEL, 0, 0) as i32
}

pub fn combo_item_text(hwnd: HWND, index: i32) -> String {
    if index < 0 {
        return String::new();
    }
    let len = send_msg(hwnd, CB_GETLBTEXTLEN, index as usize, 0);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = send_msg(hwnd, CB_GETLBTEXT, index as usize, buf.as_mut_ptr() as isize);
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

// ---------- 列表 / 列表视图 ----------

pub fn listbox_index(hwnd: HWND) -> i32 {
    send_msg(hwnd, LB_GETCURSEL, 0, 0) as i32
}

pub fn listbox_text(hwnd: HWND, index: i32) -> String {
    if index < 0 {
        return String::new();
    }
    let len = send_msg(hwnd, LB_GETTEXTLEN, index as usize, 0);
    if len <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = send_msg(hwnd, LB_GETTEXT, index as usize, buf.as_mut_ptr() as isize);
    String::from_utf16_lossy(&buf[..n.max(0) as usize])
}

/// 多选列表:选中/取消选中某一项。
pub fn listbox_set_selected(hwnd: HWND, index: i32, selected: bool) {
    if index < 0 {
        return;
    }
    send_msg(hwnd, LB_SETSEL, usize::from(selected), index as isize);
}

/// 多选列表:返回当前所有被选中项的下标。
pub fn listbox_selected_indices(hwnd: HWND) -> Vec<i32> {
    let count = send_msg(hwnd, LB_GETCOUNT, 0, 0) as i32;
    (0..count)
        .filter(|i| send_msg(hwnd, LB_GETSEL, *i as usize, 0) > 0)
        .collect()
}

/// 按文本精确查找列表框项,找不到返回 -1。
pub fn listbox_find(hwnd: HWND, text: &str) -> i32 {
    let w = Wz::new(text);
    send_msg(hwnd, LB_FINDSTRINGEXACT, usize::MAX, w.0.as_ptr() as isize) as i32
}

pub fn listview_clear(hwnd: HWND) {
    send_msg(hwnd, LVM_DELETEALLITEMS, 0, 0);
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

/// 插入一行;`cells` 为各列文本。返回行号,失败时返回负数。
pub fn listview_add_row(hwnd: HWND, cells: &[String]) -> i32 {
    let first = Wz::new(cells.first().map(String::as_str).unwrap_or(""));
    let mut item = LVITEMW {
        mask: LIST_VIEW_ITEM_FLAGS(LVIF_TEXT.0),
        iItem: i32::MAX, // 追加到末尾
        iSubItem: 0,
        state: LIST_VIEW_ITEM_STATE_FLAGS(0),
        stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
        pszText: PWSTR(first.0.as_ptr() as *mut u16),
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

    let index = send_msg(hwnd, LVM_INSERTITEMW, 0, &mut item as *mut _ as isize) as i32;
    if index < 0 {
        return index;
    }

    for (sub, text) in cells.iter().enumerate().skip(1) {
        let buf = Wz::new(text);
        let mut sub_item = LVITEMW {
            mask: LIST_VIEW_ITEM_FLAGS(LVIF_TEXT.0),
            iItem: index,
            iSubItem: sub as i32,
            state: LIST_VIEW_ITEM_STATE_FLAGS(0),
            stateMask: LIST_VIEW_ITEM_STATE_FLAGS(0),
            pszText: PWSTR(buf.0.as_ptr() as *mut u16),
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
        send_msg(
            hwnd,
            LVM_SETITEMTEXTW,
            index as usize,
            &mut sub_item as *mut _ as isize,
        );
    }

    index
}

pub fn listview_set_extended_style(hwnd: HWND, style: u32) {
    send_msg(hwnd, LVM_SETEXTENDEDLISTVIEWSTYLE, 0, style as isize);
}

/// 设置某一列的宽度(LVM_SETCOLUMNWIDTH)。
pub fn listview_set_column_width(hwnd: HWND, column: i32, width: i32) {
    send_msg(hwnd, LVM_SETCOLUMNWIDTH, column as usize, width as isize);
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
}

impl PopupMenu {
    pub fn new() -> Self {
        Self {
            handle: unsafe { CreatePopupMenu() }.unwrap_or_default(),
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
        if !self.handle.is_invalid() {
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
}

/// 命中测试:返回屏幕坐标 `screen_pt` 落在 ListView 的哪一行(负数表示没有)。
pub fn listview_item_at(list: HWND, screen_pt: POINT) -> i32 {
    let local = screen_to_client(list, screen_pt);
    let mut info = LvHitTestInfo {
        pt: local,
        flags: 0,
        i_item: -1,
        i_sub_item: 0,
    };
    send_msg(list, LVM_HITTEST, 0, &mut info as *mut _ as isize) as i32
}

/// 窗口当前是否处于最大化状态。
pub fn is_maximized(hwnd: HWND) -> bool {
    unsafe { IsZoomed(hwnd).as_bool() }
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

/// 取窗口当前 DPI(缺省 96)。
pub fn window_dpi(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}
