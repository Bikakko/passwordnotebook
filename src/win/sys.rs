//! Win32 常量与结构。
//!
//! `windows` crate 把样式常量拆成了 i32 / u32 / 新类型三种,混用时很别扭。
//! 这些都是 ABI 稳定的公开值,这里统一按 native 类型重新声明,避免类型转换噪音。

#![allow(dead_code)]

// ---------- 窗口样式 ----------
pub const WS_OVERLAPPED: u32 = 0x0000_0000;
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_POPUP: u32 = 0x8000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_DISABLED: u32 = 0x0800_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_DLGFRAME: u32 = 0x0040_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const WS_HSCROLL: u32 = 0x0010_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
pub const WS_GROUP: u32 = 0x0002_0000;
pub const WS_EX_CLIENTEDGE: u32 = 0x0000_0200;
pub const WS_EX_DLGMODALFRAME: u32 = 0x0000_0001;
pub const WS_EX_CONTROLPARENT: u32 = 0x0001_0000;
pub const WS_EX_APPWINDOW: u32 = 0x0004_0000;

pub const OVERLAPPED_WINDOW: u32 =
    WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX;
pub const DIALOG_WINDOW: u32 = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;

// ---------- 编辑框 ----------
pub const ES_LEFT: u32 = 0x0000;
pub const ES_MULTILINE: u32 = 0x0004;
pub const ES_AUTOVSCROLL: u32 = 0x0040;
pub const ES_AUTOHSCROLL: u32 = 0x0080;
pub const ES_PASSWORD: u32 = 0x0020;
pub const ES_READONLY: u32 = 0x0800;
pub const ES_WANTRETURN: u32 = 0x1000;

// ---------- 按钮 ----------
pub const BS_PUSHBUTTON: u32 = 0x0000_0000;
pub const BS_DEFPUSHBUTTON: u32 = 0x0000_0001;
pub const BS_AUTOCHECKBOX: u32 = 0x0000_0003;
pub const BS_GROUPBOX: u32 = 0x0000_0007;

// ---------- 下拉框 / 列表框 ----------
pub const CBS_DROPDOWNLIST: u32 = 0x0003;
pub const CBS_DROPDOWN: u32 = 0x0002;
pub const CBS_AUTOHSCROLL: u32 = 0x0040;
pub const LBS_NOTIFY: u32 = 0x0001;
pub const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;
pub const LBS_HASSTRINGS: u32 = 0x0040;
pub const LBS_EXTENDEDSEL: u32 = 0x0800;


// ---------- 静态文本 ----------
pub const SS_LEFT: u32 = 0x0000_0000;
pub const SS_RIGHT: u32 = 0x0000_0002;
pub const SS_CENTER: u32 = 0x0000_0001;
pub const SS_LEFTNOWORDWRAP: u32 = 0x0000_000C;

// ---------- ListView ----------
pub const LVS_REPORT: u32 = 0x0001;
pub const LVS_SINGLESEL: u32 = 0x0004;
pub const LVS_SHOWSELALWAYS: u32 = 0x0008;
pub const LVS_EX_GRIDLINES: u32 = 0x0000_0001;
pub const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;

// ---------- 窗口消息 ----------
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_SHOWWINDOW: u32 = 0x0018;
pub const WM_SETCURSOR: u32 = 0x0020;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_CTLCOLORSTATIC: u32 = 0x0138;
pub const WM_CTLCOLOREDIT: u32 = 0x0133;
pub const WM_CTLCOLORBTN: u32 = 0x0135;
pub const WM_CTLCOLORLISTBOX: u32 = 0x0134;
pub const WM_SETFONT: u32 = 0x0030;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const LVM_HITTEST: u32 = LVM_FIRST + 18;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_NOTIFY: u32 = 0x004E;
pub const WM_KEYDOWN: u32 = 0x0100;
/// 回车键(WM_KEYDOWN 的 wParam)。
pub const VK_RETURN: u32 = 0x0D;
pub const WM_INITDIALOG: u32 = 0x0110;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_APP: u32 = 0x8000;
/// 会话状态变化(需要 WTSRegisterSessionNotification)。
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;

// ---------- 控件消息 ----------
pub const BM_GETCHECK: u32 = 0x00F0;
/// 复选框被勾选时 BM_GETCHECK 的返回值。
pub const BST_CHECKED: isize = 1;
/// 对话框按 ESC 时收到的命令 id。
pub const IDCANCEL: usize = 2;
pub const BM_SETCHECK: u32 = 0x00F1;
pub const BM_CLICK: u32 = 0x00F5;

pub const CB_ADDSTRING: u32 = 0x0143;
pub const CB_RESETCONTENT: u32 = 0x014B;
pub const CB_SETCURSEL: u32 = 0x014E;
pub const CB_GETCURSEL: u32 = 0x0147;
pub const CB_GETLBTEXTLEN: u32 = 0x0149;
pub const CB_GETLBTEXT: u32 = 0x0148;
pub const CB_FINDSTRINGEXACT: u32 = 0x0158;

pub const LB_ADDSTRING: u32 = 0x0180;
pub const LB_FINDSTRINGEXACT: u32 = 0x01A2;
pub const LB_RESETCONTENT: u32 = 0x0184;
pub const LB_SETCURSEL: u32 = 0x0186;
pub const LB_GETCURSEL: u32 = 0x0188;
pub const LB_SETSEL: u32 = 0x0185;
pub const LB_GETSEL: u32 = 0x0187;
pub const LB_GETCOUNT: u32 = 0x018B;
pub const LB_GETTEXTLEN: u32 = 0x018A;
pub const LB_GETTEXT: u32 = 0x0189;
pub const LB_SETITEMDATA: u32 = 0x0199;
pub const LB_GETITEMDATA: u32 = 0x0198;

pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_SETPASSWORDCHAR: u32 = 0x00CC;
pub const EM_LIMITTEXT: u32 = 0x00C5;
pub const EM_SETREADONLY: u32 = 0x00CF;

pub const LVM_FIRST: u32 = 0x1000;
pub const LVM_DELETEALLITEMS: u32 = LVM_FIRST + 9;
pub const LVM_GETNEXTITEM: u32 = LVM_FIRST + 12;
pub const LVM_INSERTITEMW: u32 = LVM_FIRST + 77;
pub const LVM_SETITEMTEXTW: u32 = LVM_FIRST + 116;
pub const LVM_INSERTCOLUMNW: u32 = LVM_FIRST + 97;
pub const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 54;
pub const LVM_GETITEMCOUNT: u32 = LVM_FIRST + 4;
pub const LVM_ENSUREVISIBLE: u32 = LVM_FIRST + 19;
pub const LVM_SETITEMSTATE: u32 = LVM_FIRST + 43;
pub const LVM_SETCOLUMNWIDTH: u32 = LVM_FIRST + 30;

pub const LVNI_SELECTED: u32 = 0x0002;

// ---------- 标签页控件(Tab)----------
pub const TCM_FIRST: u32 = 0x1300;
pub const TCM_ADJUSTRECT: u32 = TCM_FIRST + 40;
pub const MF_POPUP: u32 = 0x0000_0010;
pub const TCM_GETCURSEL: u32 = TCM_FIRST + 11;
pub const TCM_SETCURSEL: u32 = TCM_FIRST + 12;
pub const TCM_HITTEST: u32 = TCM_FIRST + 13;
pub const TCM_GETITEMCOUNT: u32 = TCM_FIRST + 4;
pub const TCM_GETITEMW: u32 = TCM_FIRST + 60;
pub const TCM_SETITEMSIZE: u32 = TCM_FIRST + 41;
pub const WM_DRAWITEM: u32 = 0x002B;
/// 标签条控件 → 父窗口:当前选中的标签变了。
pub const TSM_TAB_CHANGED: u32 = WM_APP + 1;
pub const ODS_SELECTED: u32 = 0x0001;
pub const TCS_OWNERDRAWFIXED: u32 = 0x0400;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const TCM_DELETEALLITEMS: u32 = TCM_FIRST + 9;
pub const TCM_INSERTITEMW: u32 = TCM_FIRST + 62;
pub const TCM_SETITEMW: u32 = TCM_FIRST + 61;
/// TCN_SELCHANGE(WM_NOTIFY 的 code)。
///
/// = TCN_FIRST(-550) - 1 = -551,按 u32 表示就是 0xFFFFFDD9。
/// (之前手写成了 0xFFFFFED9,差 256,导致切换标签页的通知永远匹配不上。)
pub const TCN_SELCHANGE: u32 = 0xFFFF_FDD9;
pub const TCS_MULTILINE: u32 = 0x0200;

// 注意:LVCOLUMNW / LVITEMW 的位标志字段是 `windows` crate 的新类型,
// 因此 LVCF_* / LVIF_* / LVIS_* 一律使用 crate 里的常量,这里不再重复定义。

pub const NM_DBLCLK: i32 = -3;
pub const NM_RETURN: i32 = -4;
pub const LVN_ITEMCHANGED: i32 = -101;
pub const LVN_KEYDOWN: i32 = -155;

// ---------- 通知码 ----------
pub const BN_CLICKED: u16 = 0;
pub const EN_CHANGE: u16 = 0x0300;
pub const LBN_SELCHANGE: u16 = 1;
pub const CBN_SELCHANGE: u16 = 1;

// ---------- 会话通知 ----------
pub const WTS_SESSION_LOCK: u32 = 0x7;
pub const WTS_SESSION_UNLOCK: u32 = 0x8;
pub const NOTIFY_FOR_THIS_SESSION: u32 = 0;

// ---------- 其它 ----------
pub const CW_USEDEFAULT: i32 = -2147483648;
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWMINIMIZED: i32 = 2;
pub const SW_SHOWMAXIMIZED: i32 = 3;
pub const SW_HIDE: i32 = 0;
pub const SW_RESTORE: i32 = 9;
pub const SW_MAXIMIZE: i32 = 3;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_SHOWWINDOW: u32 = 0x0040;

pub const MB_OK: u32 = 0x0000_0000;
pub const MB_OKCANCEL: u32 = 0x0000_0001;
pub const MB_YESNO: u32 = 0x0000_0004;
pub const MB_ICONERROR: u32 = 0x0000_0010;
pub const MB_ICONWARNING: u32 = 0x0000_0030;
pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
pub const MB_ICONQUESTION: u32 = 0x0000_0020;
pub const MB_DEFBUTTON2: u32 = 0x0000_0100;
pub const IDOK: i32 = 1;
pub const IDYES: i32 = 6;
pub const IDNO: i32 = 7;

pub const GWLP_USERDATA: i32 = -21;

pub const DEFAULT_CHARSET: u32 = 1;
pub const CLEARTYPE_QUALITY: u32 = 5;
pub const FW_NORMAL: i32 = 400;
pub const FW_SEMIBOLD: i32 = 600;

pub const COLOR_WINDOW: i32 = 5;
pub const TRANSPARENT: i32 = 1;
pub const DT_LEFT: u32 = 0x0000_0000;

pub const SPI_GETSCREENSAVEACTIVE: u32 = 0x0072;
pub const SPI_GETSCREENSAVETIMEOUT: u32 = 0x000E;
