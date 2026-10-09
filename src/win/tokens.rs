//! 界面设计令牌:间距、尺寸、颜色的唯一来源。
//!
//! 全部为 **96-DPI 逻辑像素**,使用时经 [`super::ui::scale`] 换算。
//! 新增控件时优先从这里取值,不要再写裸数字。

// ---------- 间距 ----------
/// 窗口/对话框内容的外边距。
pub const MARGIN: i32 = 20;

// ---------- 尺寸 ----------
pub const LABEL_H: i32 = 22;
pub const FIELD_H: i32 = 30;
pub const CHECK_H: i32 = 24;
pub const BUTTON_H: i32 = 36;
/// 标准对话框按钮(`确定`/`取消`/`保存`/`关闭`/`完成`/`编辑…`)的统一宽度。
pub const BUTTON_W: i32 = 100;
/// 下拉框的展开高度(控件自身高度由系统按字体决定,这里只给足余量)。
pub const COMBO_DROP_H: i32 = 200;
/// 主列表行高。
pub const LIST_ROW_H: i32 = 30;
/// 两行状态栏的高度(计数一行、路径与警告一行)。
/// 按行高约 21px 留两行,再留一点余量,免得第二行被裁掉。
pub const STATUS_H: i32 = 44;

// ---------- 区块 ----------
pub const SECTION_TITLE_H: i32 = 26;

// ---------- 颜色(COLORREF,0x00BBGGRR)----------
/// 正文色 #1F2430。
pub const TEXT: u32 = 0x0030_241F;
/// 错误/警告色 #C0392B。
pub const ERROR_TEXT: u32 = 0x002B_39C0;
/// 次要说明色(空状态提示等)#999EA1。
pub const MUTED_TEXT: u32 = 0x00A1_9E99;
/// 列表网格线(极淡:向 Web 风分隔线的观感看齐,不抢内容)。
pub const GRIDLINE: u32 = 0x00EC_E8E5;
/// 列表列头的背景色(浅灰,与白色内容行区分)。
pub const HEADER_BG: u32 = 0x00EC_ECEC;
/// 只读字段的背景色(浅灰:凹陷边框+白底是「可编辑」的样子,平底灰才读作只读)。
pub const READONLY_BG: u32 = 0x00F0_F0F0;

// ---------- 主色与状态 ----------
/// 主题强调色 #2F6FEB:页签下划线与自绘主色按钮。
pub const ACCENT: u32 = 0x00EB_6F2F;
/// 主色按钮的悬停 / 按下底色。
pub const ACCENT_HOVER: u32 = 0x00D1_622A; // #2A62D1
pub const ACCENT_PRESSED: u32 = 0x00B0_5324; // #2453B0
/// 主色按钮上的文字。
pub const ACCENT_TEXT: u32 = 0x00FF_FFFF;
/// 主色按钮禁用时的底色与文字。
pub const ACCENT_DISABLED_BG: u32 = 0x00EA_E6E4; // #E4E6EA
pub const ACCENT_DISABLED_TEXT: u32 = 0x00B6_AEA9; // #A9AEB6
/// 圆角半径(逻辑像素):按钮 8、文本字段 8(与按钮同档)。
pub const RADIUS_BTN: i32 = 8;
pub const RADIUS_FIELD: i32 = 8;
/// 文本字段的描边色(获得焦点时改用 ACCENT)。
pub const FIELD_BORDER: u32 = 0x00E0_DAD6; // #D6DAE0
/// 次级(白底描边)按钮:悬停 / 按下 / 禁用。
pub const BTN_HOVER_BG: u32 = 0x00F7_F4F3; // #F3F4F7
pub const BTN_PRESSED_BG: u32 = 0x00F0_EBE9; // #E9EBF0
pub const BTN_DISABLED_BG: u32 = 0x00F7_F6F5; // #F5F6F7
pub const BTN_DISABLED_TEXT: u32 = 0x00B6_AEA9; // #A9AEB6
/// 密码强度行的取色(「弱」复用 ERROR_TEXT)。
pub const STRENGTH_MID: u32 = 0x0000_77C7; // #C77700
pub const STRENGTH_STRONG: u32 = 0x003E_8E1E; // #1E8E3E
/// 列表「标签」列的微标签:描边与文字。
pub const CHIP_BORDER: u32 = 0x00E3_DCD8; // #D8DCE3
pub const CHIP_TEXT: u32 = 0x0077_706A; // #6A7077
/// 空状态大图标的颜色(浅灰,不与正文抢注意力)。
pub const EMPTY_ICON: u32 = 0x00D2_CDC8; // #C8CDD2

// 自绘分类页签。
pub const STRIP_BG: u32 = 0x00F5_F5F5; // #F5F5F5 未选中的底色
pub const STRIP_CARD_BG: u32 = 0x00FF_FFFF; // 选中:白色卡片
pub const STRIP_TEXT: u32 = 0x0046_4646; // #464646
pub const STRIP_TEXT_ON: u32 = 0x0016_1616;
pub const STRIP_SEPARATOR: u32 = 0x00DC_DCDC; // #DCDCDC
pub const STRIP_HOVER_BG: u32 = 0x00EC_ECEC; // 未选中页签的悬停底色
pub const STRIP_ACCENT: u32 = ACCENT; // 选中下划线(与主色按钮同色)

// ---------- 文本 ----------
/// 敏感字段的遮蔽字符(详情弹窗、主界面的解锁/创建表单共用)。
pub const MASK_CHAR: char = '\u{25CF}';
/// 未填写分类的条目在界面上的显示名(列表、编辑器下拉、右键菜单共用)。
pub const UNCATEGORIZED: &str = "未分类";
