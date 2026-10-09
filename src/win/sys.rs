//! Win32 常量与结构。
//!
//! `windows` crate 把样式常量拆成了 i32 / u32 / 新类型三种,混用时很别扭。
//! 这些都是 ABI 稳定的公开值,这里统一按 native 类型重新声明,避免类型转换噪音。

// ---------- 窗口样式 ----------
pub const WS_OVERLAPPED: u32 = 0x0000_0000;
pub const WS_CAPTION: u32 = 0x00C0_0000;
pub const WS_SYSMENU: u32 = 0x0008_0000;
pub const WS_THICKFRAME: u32 = 0x0004_0000;
pub const WS_MINIMIZEBOX: u32 = 0x0002_0000;
pub const WS_MAXIMIZEBOX: u32 = 0x0001_0000;
pub const WS_CHILD: u32 = 0x4000_0000;
pub const WS_VISIBLE: u32 = 0x1000_0000;
pub const WS_CLIPCHILDREN: u32 = 0x0200_0000;
pub const WS_CLIPSIBLINGS: u32 = 0x0400_0000;
pub const WS_BORDER: u32 = 0x0080_0000;
pub const WS_VSCROLL: u32 = 0x0020_0000;
pub const WS_HSCROLL: u32 = 0x0010_0000;
pub const WS_TABSTOP: u32 = 0x0001_0000;
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
/// 按钮样式的低 4 位(BS_TYPEMASK):用来区分推送按钮与复选框/分组框。
pub const BS_TYPEMASK: u32 = 0x0000_000F;

// ---------- 下拉框 / 列表框 ----------
pub const CBS_DROPDOWNLIST: u32 = 0x0003;
pub const LBS_NOTIFY: u32 = 0x0001;
/// 自绘列表项(固定行高,配合 WM_MEASUREITEM / WM_DRAWITEM)。
pub const LBS_OWNERDRAWFIXED: u32 = 0x0010;
/// 让列表框继续保存项文本(自绘时仍需 LB_GETTEXT 取标签)。
pub const LBS_HASSTRINGS: u32 = 0x0040;
pub const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;

// ---------- 静态文本 ----------
pub const SS_LEFT: u32 = 0x0000_0000;
pub const SS_CENTER: u32 = 0x0000_0001;
/// 让静态控件把点击通知(WM_COMMAND/STN_CLICKED)发给父窗口。
pub const SS_NOTIFY: u32 = 0x0100;

// ---------- ListView ----------
pub const LVS_REPORT: u32 = 0x0001;
pub const LVS_SINGLESEL: u32 = 0x0004;
pub const LVS_SHOWSELALWAYS: u32 = 0x0008;
/// 虚拟列表:行文本由父窗口按需提供(`LVN_GETDISPINFO`),不预先插入。
pub const LVS_OWNERDATA: u32 = 0x1000;
pub const LVS_EX_GRIDLINES: u32 = 0x0000_0001;
pub const LVS_EX_FULLROWSELECT: u32 = 0x0000_0020;

// ---------- 窗口消息 ----------
pub const WM_DRAWITEM: u32 = 0x002B;
pub const WM_MEASUREITEM: u32 = 0x002C;
pub const WM_CREATE: u32 = 0x0001;
pub const WM_DESTROY: u32 = 0x0002;
pub const WM_SIZE: u32 = 0x0005;
pub const WM_SETFOCUS: u32 = 0x0007;
pub const WM_KILLFOCUS: u32 = 0x0008;
pub const WM_ENABLE: u32 = 0x000A;
pub const WM_CLOSE: u32 = 0x0010;
pub const WM_PAINT: u32 = 0x000F;
pub const WM_ERASEBKGND: u32 = 0x0014;
pub const WM_GETMINMAXINFO: u32 = 0x0024;
pub const WM_CTLCOLORSTATIC: u32 = 0x0138;
pub const WM_SETFONT: u32 = 0x0030;
pub const WM_GETFONT: u32 = 0x0031;
pub const WM_COMMAND: u32 = 0x0111;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const LVM_HITTEST: u32 = LVM_FIRST + 18;
pub const WM_TIMER: u32 = 0x0113;
pub const WM_NOTIFY: u32 = 0x004E;
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_MOUSEMOVE: u32 = 0x0200;
pub const WM_LBUTTONDOWN: u32 = 0x0201;
pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_LBUTTONDBLCLK: u32 = 0x0203;
pub const WM_MOUSELEAVE: u32 = 0x02A3;
pub const WM_COPY: u32 = 0x0301;
pub const WM_VSCROLL: u32 = 0x0115;
pub const WM_MOUSEWHEEL: u32 = 0x020A;
pub const WM_NCDESTROY: u32 = 0x0082;

/// WM_VSCROLL 的低位字(滚动条通知码)。
pub const SB_LINEUP: usize = 0;
pub const SB_LINEDOWN: usize = 1;
pub const SB_PAGEUP: usize = 2;
pub const SB_PAGEDOWN: usize = 3;
pub const SB_THUMBPOSITION: usize = 4;
pub const SB_THUMBTRACK: usize = 5;
/// 回车键(WM_KEYDOWN 的 wParam)。
pub const VK_RETURN: u32 = 0x0D;
/// 空格键(按钮键盘激活)。
pub const VK_SPACE: u32 = 0x20;
pub const WM_DPICHANGED: u32 = 0x02E0;
pub const WM_APP: u32 = 0x8000;
/// 会话状态变化(需要 WTSRegisterSessionNotification)。
pub const WM_WTSSESSION_CHANGE: u32 = 0x02B1;

// ---------- 控件消息 ----------
pub const BM_GETCHECK: u32 = 0x00F0;
/// 对话框按 ESC 时收到的命令 id。
pub const IDCANCEL: usize = 2;
pub const BM_SETCHECK: u32 = 0x00F1;

pub const CB_ADDSTRING: u32 = 0x0143;
pub const CB_SETCURSEL: u32 = 0x014E;
pub const CB_GETCURSEL: u32 = 0x0147;
pub const CB_GETLBTEXTLEN: u32 = 0x0149;
pub const CB_GETLBTEXT: u32 = 0x0148;

pub const LB_ADDSTRING: u32 = 0x0180;
pub const LB_FINDSTRINGEXACT: u32 = 0x01A2;
pub const LB_RESETCONTENT: u32 = 0x0184;
pub const LB_SETCURSEL: u32 = 0x0186;
pub const LB_GETCURSEL: u32 = 0x0188;
pub const LB_GETTEXTLEN: u32 = 0x018A;
pub const LB_GETTEXT: u32 = 0x0189;
/// 列表框的横向滚动范围(配合 WS_HSCROLL)。
pub const LB_SETHORIZONTALEXTENT: u32 = 0x0194;

pub const EM_SETSEL: u32 = 0x00B1;
pub const EM_SETPASSWORDCHAR: u32 = 0x00CC;
/// 给编辑框设置占位提示文本(wParam=1 表示获得焦点时也显示)。
pub const EM_SETCUEBANNER: u32 = 0x1501;

pub const LVM_FIRST: u32 = 0x1000;
pub const LVM_GETNEXTITEM: u32 = LVM_FIRST + 12;
pub const LVM_INSERTCOLUMNW: u32 = LVM_FIRST + 97;
pub const LVM_SETEXTENDEDLISTVIEWSTYLE: u32 = LVM_FIRST + 54;
pub const LVM_SETITEMCOUNT: u32 = LVM_FIRST + 47;
pub const LVM_SETITEMSTATE: u32 = LVM_FIRST + 43;
/// 取某一列表项的状态位(LVM_FIRST + 44)。
pub const LVM_GETITEMSTATE: u32 = LVM_FIRST + 44;
/// 列表项状态:被选中(LVM_GETITEMSTATE 的掩码)。
pub const LVIS_SELECTED: u32 = 0x0002;
pub const LVM_GETCOLUMNWIDTH: u32 = LVM_FIRST + 29;
pub const LVM_SETCOLUMNWIDTH: u32 = LVM_FIRST + 30;
pub const LVM_GETHEADER: u32 = LVM_FIRST + 31;
pub const LVM_SETIMAGELIST: u32 = LVM_FIRST + 3;
pub const LVSIL_SMALL: usize = 1;

pub const LVNI_SELECTED: u32 = 0x0002;

// ---------- 自绘标签条与文本绘制 ----------
/// 标签条控件 → 父窗口:当前选中的标签变了。
pub const TSM_TAB_CHANGED: u32 = WM_APP + 1;
/// 列表列宽变动 → 父窗口:用户调整了主列表的列宽。
pub const TSM_COLUMN_RESIZED: u32 = WM_APP + 2;
pub const TCS_MULTILINE: u32 = 0x0200;
pub const DT_CENTER: u32 = 0x0001;
pub const DT_RIGHT: u32 = 0x0002;
pub const DT_VCENTER: u32 = 0x0004;
pub const DT_LEFT: u32 = 0x0000_0000;
pub const DT_SINGLELINE: u32 = 0x0020;
pub const DT_END_ELLIPSIS: u32 = 0x0000_8000;

// 注意:LVCOLUMNW / LVITEMW 的位标志字段是 `windows` crate 的新类型,
// 因此 LVCF_* / LVIF_* / LVIS_* 一律使用 crate 里的常量,这里不再重复定义。

pub const NM_CUSTOMDRAW: i32 = -12;
pub const NM_DBLCLK: i32 = -3;
/// 虚拟列表绘制时向父窗口索取某行某列的文本(= LVN_FIRST - 77)。
pub const LVN_GETDISPINFOW: i32 = -177;

// ---------- 列表头控件(Header) ----------
/// 列头的列数(HDM_FIRST + 0)。
pub const HDM_GETITEMCOUNT: u32 = 0x1200;
/// 取列头某一列的矩形(HDM_FIRST + 7)。
pub const HDM_GETITEMRECT: u32 = 0x1207;
/// 取列头某一列的文本与格式(HDM_FIRST + 11)。
pub const HDM_GETITEMW: u32 = 0x120B;

// ---------- 列表头控件(Header)通知 ----------
pub const HDN_DIVIDERDBLCLICKA: i32 = -305;
pub const HDN_DIVIDERDBLCLICKW: i32 = -325;
pub const HDN_ENDTRACKA: i32 = -307;
pub const HDN_ENDTRACKW: i32 = -327;

// ---------- 通知码 ----------
pub const BN_CLICKED: u16 = 0;
pub const BN_SETFOCUS: u16 = 6;
pub const EN_CHANGE: u16 = 0x0300;
pub const LBN_SELCHANGE: u16 = 1;
pub const CBN_SELCHANGE: u16 = 1;

// ---------- 会话通知 ----------
pub const WTS_SESSION_LOCK: u32 = 0x7;
pub const NOTIFY_FOR_THIS_SESSION: u32 = 0;

// ---------- 其它 ----------
pub const SW_SHOW: i32 = 5;
pub const SW_SHOWMINIMIZED: i32 = 2;
pub const SW_SHOWMAXIMIZED: i32 = 3;
pub const SW_HIDE: i32 = 0;
pub const SW_RESTORE: i32 = 9;
pub const SW_SHOWNORMAL: i32 = 1;
pub const SWP_NOZORDER: u32 = 0x0004;
pub const SWP_NOSIZE: u32 = 0x0001;
pub const SWP_NOMOVE: u32 = 0x0002;
pub const SWP_NOACTIVATE: u32 = 0x0010;

pub const MB_OK: u32 = 0x0000_0000;
pub const MB_YESNO: u32 = 0x0000_0004;
pub const MB_ICONERROR: u32 = 0x0000_0010;
pub const MB_ICONWARNING: u32 = 0x0000_0030;
pub const MB_ICONINFORMATION: u32 = 0x0000_0040;
pub const MB_ICONQUESTION: u32 = 0x0000_0020;
pub const MB_DEFBUTTON2: u32 = 0x0000_0100;
pub const IDYES: i32 = 6;

pub const GWLP_USERDATA: i32 = -21;
pub const GWL_STYLE: i32 = -16;

pub const SPI_GETSCREENSAVEACTIVE: u32 = 0x0072;
pub const SPI_GETSCREENSAVETIMEOUT: u32 = 0x000E;
