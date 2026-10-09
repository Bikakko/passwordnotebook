//! 导入 / 导出:CSV 与 JSON。
//!
//! 两条通道的分工是刻意分开的:
//!
//! - **CSV 面向互操作**:固定 9 列(0.3.0 起在 `Url` 后多了 `ApiKey,ApiEndpoint`),
//!   Excel 可直接打开;导入时会按别名表识别 Chrome / Edge / Bitwarden / 1Password /
//!   KeePass / LastPass 等常见导出文件的表头,不必手工改列名。
//! - **JSON 面向备份**:完整保留 id、分类、标签与时间戳,便于原样还原。
//!
//! 两个必须记住的安全事实:
//!
//! 1. **导出是明文的**。密码是否写进文件由调用方通过 [`ExportOptions`] 显式决定,
//!    本模块不会「顺手」把密码塞进去。
//! 2. **导入是破坏性的**(覆盖策略会改动已有条目)。因此 [`crate::vault::VaultService::import_entries`]
//!    只做一次落盘,并且调用方应在导入前先备份原文件。

use std::path::Path;

use serde::Serialize;
use serde_json::{Map, Value};
use zeroize::Zeroizing;

use crate::error::{Result, VaultError};
use crate::model::{now_secs, Document, Entry};

/// JSON 导出文件的格式标记。
pub const JSON_FORMAT_TAG: &str = "password-notebook";
/// JSON 导出文件的格式版本。
pub const JSON_FORMAT_VERSION: u32 = 1;

/// CSV 的规范表头(导出时使用;导入时只认别名表,不要求顺序或大小写一致)。
///
/// 0.3.0 起在 `Url` 后插入 `ApiKey,ApiEndpoint`;`Password` 与 `ApiKey` 两列
/// 随「包含明文密码与 API 密钥」选项成对出现或成对省略。认自家旧文件
/// (0.2.x 的 7 列)的逻辑见 [`is_own_export`]。
pub const CSV_HEADER: [&str; 9] = [
    "Title", "Username", "Password", "Url", "ApiKey", "ApiEndpoint", "Category", "Tags", "Notes",
];

/// 标签在 CSV 单元格内的分隔符(`|` 也接受)。
pub const TAG_SEPARATOR: char = ';';

/// 单次导入允许的最大文件体积(64 MiB)。
///
/// 导入全程是「整份读进内存 → 展开成 `Vec<Vec<String>>` → 再逐行转成条目」,
/// 峰值内存约为文件体积的好几倍。个人密码管理器实际用到的文件通常在几百 KB,
/// 这个上限留了两个数量级的余量,但能挡住误选到一个超大文件 / 被构造的输入
/// 把内存撑爆(release 构建 `panic = "abort"`,耗尽时是直接退出,没有提示)。
pub const MAX_IMPORT_BYTES: u64 = 64 * 1024 * 1024;

/// 单次导入允许的最大条目数。
///
/// 与体积上限互补:体积够小也能塞下海量空行 / 极短行(几百万条「只有一个字符的行」),
/// 而每条都要变成一个 `Entry`,构造它们的开销远超文件本身。
pub const MAX_IMPORT_ENTRIES: usize = 100_000;

/// 体积上限的提示文案(带上实际大小,方便判断是超了一点还是差很远)。
fn too_large(actual: u64) -> VaultError {
    VaultError::Invalid(format!(
        "文件太大了（{}），单个文件最多 {}。请确认选中的是密码本文件。",
        human_size(actual),
        human_size(MAX_IMPORT_BYTES),
    ))
}

fn too_many_entries(actual: usize) -> VaultError {
    VaultError::Invalid(format!(
        "文件里条目太多（{} 条），一次最多导入 {} 条。",
        actual, MAX_IMPORT_ENTRIES
    ))
}

/// 把字节数说成人话,避免界面里出现「62914560 字节」这种没法判断的数字。
///
/// 纯整数实现:输出与旧的 `{:.1}` 浮点写法逐项一致(四舍五入、平局取偶),
/// 同时把整套 f64 格式化机器挡在二进制外(约占 15 KB)。
fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let num = bytes as u128;
    let mut unit = 0usize;
    while unit + 1 < UNITS.len() && num >= 1024u128.pow(unit as u32 + 1) {
        unit += 1;
    }
    let den = 1024u128.pow(unit as u32);
    let round = |n: u128| -> u128 {
        let (q, r) = (n / den, n % den);
        let twice = r * 2;
        // 四舍五入;平局时向偶数取整(与 `{:.0}` / `{:.1}` 的口径一致)。
        if twice > den || (twice == den && q % 2 == 1) {
            q + 1
        } else {
            q
        }
    };
    // 整数值不打小数点(B / KB 走这条路),否则保留一位。
    if num >= 10 * den || num.is_multiple_of(den) {
        format!("{} {}", round(num), UNITS[unit])
    } else {
        let tenths = round(num * 10);
        format!("{}.{} {}", tenths / 10, tenths % 10, UNITS[unit])
    }
}

/// 导出格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// 逗号分隔,UTF-8 带 BOM,Excel 友好。
    Csv,
    /// 自描述的 JSON 信封,备份友好。
    Json,
}

impl Format {
    /// 下拉框里的呈现顺序:填充与解析共用这一份,不会各写一遍再对不上。
    pub const ALL: [Format; 2] = [Format::Csv, Format::Json];

    /// 下拉框下标 → 格式。未知值按 CSV。
    pub fn from_combo(index: i32) -> Self {
        Self::ALL.get(index.max(0) as usize).copied().unwrap_or(Format::Csv)
    }

    /// 文件扩展名(不含点)。
    pub fn extension(self) -> &'static str {
        match self {
            Format::Csv => "csv",
            Format::Json => "json",
        }
    }

    /// 界面下拉框里的文字。
    pub fn label(self) -> &'static str {
        match self {
            Format::Csv => "CSV（Excel 可直接打开）",
            Format::Json => "JSON（完整备份）",
        }
    }

    /// 供文件对话框使用的过滤器。
    pub fn filter(self) -> (&'static str, &'static str) {
        match self {
            Format::Csv => ("CSV 文件 (*.csv)", "*.csv"),
            Format::Json => ("JSON 文件 (*.json)", "*.json"),
        }
    }
}

/// 导出选项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportOptions {
    /// 是否把密码与 API 密钥写成明文。关闭时两者的列/字段整体省略,而不是留空 ——
    /// 留空会在「覆盖导入」时被误读成「把原值清空」。
    pub include_secrets: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_secrets: true,
        }
    }
}

/// 遇到同名(标题 + 用户名相同)条目时的处理方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DuplicateStrategy {
    /// 一律新增,不做任何比对。
    Append,
    /// 已有同名条目则跳过导入项。
    Skip,
    /// 已有同名条目则用导入项覆盖它(密码/API 密钥为空时不覆盖原值)。
    Overwrite,
}

impl DuplicateStrategy {
    /// 下拉框里的呈现顺序:跳过 / 覆盖 / 全部新增。
    pub const ALL: [DuplicateStrategy; 3] = [
        DuplicateStrategy::Skip,
        DuplicateStrategy::Overwrite,
        DuplicateStrategy::Append,
    ];

    /// 下拉框下标 → 重复项处理方式。未知值按「跳过重复项」。
    pub fn from_combo(index: i32) -> Self {
        Self::ALL
            .get(index.max(0) as usize)
            .copied()
            .unwrap_or(DuplicateStrategy::Skip)
    }

    pub fn label(self) -> &'static str {
        match self {
            DuplicateStrategy::Append => "全部新增",
            DuplicateStrategy::Skip => "跳过重复项",
            DuplicateStrategy::Overwrite => "用文件内容覆盖",
        }
    }
}

/// 导入结果统计。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImportOutcome {
    pub added: usize,
    pub updated: usize,
    pub skipped: usize,
}

impl ImportOutcome {
    /// 是否真的改动了库(没改动就不必落盘)。
    pub fn changed(&self) -> bool {
        self.added + self.updated > 0
    }

    /// 面向用户的一句话总结。
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.added > 0 {
            parts.push(format!("新增 {} 条", self.added));
        }
        if self.updated > 0 {
            parts.push(format!("覆盖 {} 条", self.updated));
        }
        if self.skipped > 0 {
            parts.push(format!("跳过 {} 条", self.skipped));
        }
        if parts.is_empty() {
            "没有可导入的条目。".to_string()
        } else {
            parts.join("、")
        }
    }
}

// ---------- 导出 ----------

/// 把整库导出成文本。返回 `Zeroizing<String>`:内容是明文,离开作用域即被抹掉。
pub fn export(document: &Document, format: Format, options: ExportOptions) -> Result<Zeroizing<String>> {
    match format {
        Format::Csv => Ok(Zeroizing::new(export_csv(document, options))),
        Format::Json => export_json(document, options),
    }
}

/// 导出到文件,返回导出的条目数。
pub fn export_to_path(
    document: &Document,
    path: &Path,
    format: Format,
    options: ExportOptions,
) -> Result<usize> {
    let text = export(document, format, options)?;
    std::fs::write(path, text.as_bytes())?;
    Ok(active_entries(document).len())
}

/// 从文件读取并解析,返回待导入的条目。
///
/// 编码只接受 UTF-8(带不带 BOM 都行)。Excel 在中文 Windows 上默认另存为 GBK,
/// 那种文件会得到一个明确的错误提示,而不是一堆乱码条目 —— 见 `docs/limitations.md`。
///
/// 体积上限在**读取之前**按文件元数据判断,不合规的文件一个字节都不会进内存;
/// 判断不了大小(文件刚被删、网络盘、某些文件系统不填 `len`)就放行,
/// 由 [`parse`] 里的条数上限兜底。
pub fn import_from_path(path: &Path) -> Result<Vec<Entry>> {
    match std::fs::metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() > MAX_IMPORT_BYTES => return Err(too_large(meta.len())),
        _ => {}
    }
    // 文件内容是明文(可能含密码),读完立刻纳入 Zeroizing 管理。
    let bytes = Zeroizing::new(std::fs::read(path)?);
    let text = decode_utf8(bytes.as_slice())?;
    parse(&text, None)
}

/// 按内容解析导入文本;`hint` 为 `None` 时自动判断 JSON / CSV。
///
/// 体积上限在这里**也**判一次,而不是只靠 [`import_from_path`]:调用方可能已经
/// 把内容读进内存(自检、将来的粘贴导入),那时再判体积只是拒绝得快一点,
/// 但至少不会一路解析到底。
pub fn parse(text: &str, hint: Option<Format>) -> Result<Vec<Entry>> {
    let trimmed = text.trim_start_matches('\u{feff}');
    if trimmed.trim().is_empty() {
        return Err(VaultError::Invalid("文件是空的。".into()));
    }
    if text.len() as u64 > MAX_IMPORT_BYTES {
        return Err(too_large(text.len() as u64));
    }
    match hint.unwrap_or_else(|| detect_format(trimmed)) {
        Format::Json => parse_json(trimmed),
        // CSV 解析器自己会处理 BOM,并且要靠它判断「这是不是本程序导出的文件」。
        Format::Csv => parse_csv(text),
    }
}

/// 按首个非空白字符判断格式。
pub fn detect_format(text: &str) -> Format {
    let head = text.trim_start_matches('\u{feff}').trim_start();
    if head.starts_with('{') || head.starts_with('[') {
        Format::Json
    } else {
        Format::Csv
    }
}

fn decode_utf8(bytes: &[u8]) -> Result<Zeroizing<String>> {
    match std::str::from_utf8(bytes) {
        Ok(text) => Ok(Zeroizing::new(text.to_string())),
        Err(e) => Err(VaultError::Invalid(format!(
            "文件不是 UTF-8 编码（第 {} 字节）。请在 Excel 里用「CSV UTF-8（逗号分隔）」重新保存，或转成 UTF-8 再导入。",
            e.valid_up_to() + 1
        ))),
    }
}

/// 导出顺序:按标题、再按用户名(均忽略大小写)排序,结果与库内插入顺序无关。
fn active_entries(document: &Document) -> Vec<&Entry> {
    let mut out: Vec<&Entry> = document.entries.iter().filter(|e| !e.is_deleted()).collect();
    out.sort_by(|a, b| {
        lower(&a.title)
            .cmp(&lower(&b.title))
            .then_with(|| lower(&a.username).cmp(&lower(&b.username)))
    });
    out
}

fn lower(value: &str) -> String {
    value.trim().to_lowercase()
}

/// CSV 不承载收藏状态,表头固定为 [`CSV_HEADER`];`Password` 与 `ApiKey` 两列
/// 随导出选项成对省略——不含凭据的导出不该漏出任何一种。要连收藏一起备份,
/// 用 JSON 或加密的 `.pkk` 副本。
fn export_csv(document: &Document, options: ExportOptions) -> String {
    // 带 BOM:否则 Excel 会把中文按本地代码页解读成乱码。
    let mut out = String::from("\u{feff}");
    let header: Vec<&str> = CSV_HEADER
        .iter()
        .copied()
        .filter(|c| options.include_secrets || !matches!(*c, "Password" | "ApiKey"))
        .collect();
    out.push_str(&header.join(","));
    out.push_str("\r\n");

    for entry in active_entries(document) {
        let mut cells: Vec<String> = vec![
            entry.title.clone(),
            entry.username.clone(),
        ];
        if options.include_secrets {
            cells.push(entry.password.as_str().to_string());
        }
        cells.push(entry.url.clone());
        if options.include_secrets {
            cells.push(entry.api_key.as_str().to_string());
        }
        cells.push(entry.api_endpoint.clone());
        cells.push(entry.category.clone());
        cells.push(entry.tags.join(&TAG_SEPARATOR.to_string()));
        cells.push(entry.notes.clone());

        let line: Vec<String> = cells.iter().map(|c| csv_cell(c)).collect();
        out.push_str(&line.join(","));
        out.push_str("\r\n");
    }
    out
}

/// 单个 CSV 单元格:先做公式注入防护,再按 RFC 4180 决定是否加引号。
fn csv_cell(value: &str) -> String {
    let guarded = guard_formula(value);
    if guarded.contains([',', '"', '\r', '\n']) {
        format!("\"{}\"", guarded.replace('"', "\"\""))
    } else {
        guarded
    }
}

/// CSV 公式注入防护。
///
/// Excel 会把以 `=` `+` `-` `@` 开头的单元格当公式执行,而标题/备注是用户可控的
/// (可能来自别人给的 CSV)。按 OWASP 的建议加一个前导单引号 ——
/// 导入时 [`unguard_formula`] 会把它去掉,因此本程序自身的往返不受影响。
///
/// 前导单引号本身也要转义,否则「密码恰好以 `'=` 开头」这类值会被 [`unguard_formula`]
/// 误当成防护前缀吃掉一个字符。
fn guard_formula(value: &str) -> String {
    if is_guard_char(value.chars().next()) {
        format!("'{value}")
    } else {
        value.to_string()
    }
}

/// 去掉 [`guard_formula`] 加上的前导单引号。
fn unguard_formula(value: &str) -> String {
    let mut chars = value.chars();
    if chars.next() == Some('\'') && is_guard_char(chars.next()) {
        return value[1..].to_string();
    }
    value.to_string()
}

/// 需要防护的首字符(含单引号自身 —— 它同时是防护前缀)。
fn is_guard_char(c: Option<char>) -> bool {
    c.is_some_and(|c| matches!(c, '=' | '+' | '-' | '@' | '\t' | '\r' | '\''))
}

#[derive(Serialize)]
struct ExportEntry<'a> {
    id: &'a str,
    title: &'a str,
    username: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    password: Option<&'a str>,
    url: &'a str,
    /// 与密码同进退:关闭「包含明文密码与 API 密钥」时不写出。
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<&'a str>,
    api_endpoint: &'a str,
    category: &'a str,
    tags: &'a [String],
    /// JSON 里显式写出收藏状态(CSV 没有这一列,见 [`export_csv`])。
    favorite: bool,
    notes: &'a str,
    created: i64,
    updated: i64,
}

#[derive(Serialize)]
struct ExportDocument<'a> {
    format: &'static str,
    version: u32,
    exported_at: i64,
    categories: &'a [String],
    tags: &'a [String],
    entries: Vec<ExportEntry<'a>>,
}

fn export_json(document: &Document, options: ExportOptions) -> Result<Zeroizing<String>> {
    let entries = active_entries(document)
        .into_iter()
        .map(|e| ExportEntry {
            id: &e.id,
            title: &e.title,
            username: &e.username,
            password: options.include_secrets.then(|| e.password.as_str()),
            url: &e.url,
            api_key: options.include_secrets.then(|| e.api_key.as_str()),
            api_endpoint: &e.api_endpoint,
            category: &e.category,
            tags: &e.tags,
            favorite: e.favorite,
            notes: &e.notes,
            created: e.created,
            updated: e.updated,
        })
        .collect();

    let payload = ExportDocument {
        format: JSON_FORMAT_TAG,
        version: JSON_FORMAT_VERSION,
        exported_at: now_secs(),
        categories: &document.categories,
        tags: &document.tags,
        entries,
    };

    // 直接序列化到字符串,不经过 Value 树 —— 少一份明文副本。
    Ok(Zeroizing::new(serde_json::to_string_pretty(&payload)?))
}

// ---------- 导入 ----------

/// 可识别的字段。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Id,
    Title,
    Username,
    Password,
    Url,
    ApiKey,
    ApiEndpoint,
    Category,
    Tags,
    Favorite,
    Notes,
    Created,
    Updated,
}

const TITLE_ALIASES: &[&str] = &["title", "name", "account", "item", "entry", "标题", "名称", "账号名", "项目"];
const USERNAME_ALIASES: &[&str] = &[
    "username", "user", "login", "loginname", "loginusername", "accountname", "用户名", "登录名", "账号", "账户",
    "使用者",
];
const PASSWORD_ALIASES: &[&str] = &["password", "pass", "passwd", "loginpassword", "密码", "口令"];
const URL_ALIASES: &[&str] = &[
    "url", "uri", "website", "websiteurl", "weburl", "loginuri", "link", "网址", "网站", "链接", "登录网址",
];
const API_KEY_ALIASES: &[&str] = &["apikey", "api密钥", "密钥"];
const API_ENDPOINT_ALIASES: &[&str] = &["apiendpoint", "endpoint", "api端点", "端点"];
const CATEGORY_ALIASES: &[&str] = &["category", "folder", "group", "grouping", "分类", "分组", "文件夹", "目录"];
const TAGS_ALIASES: &[&str] = &["tags", "tag", "labels", "label", "标签"];
const FAVORITE_ALIASES: &[&str] = &[
    "favorite", "favourite", "fav", "starred", "pinned", "收藏", "收藏夹", "置顶",
];
const NOTES_ALIASES: &[&str] = &["notes", "note", "comment", "comments", "extra", "memo", "备注", "说明", "注释"];
const ID_ALIASES: &[&str] = &["id", "uuid", "标识"];
const CREATED_ALIASES: &[&str] = &["created", "createdat", "creationtime", "创建时间", "创建于"];
const UPDATED_ALIASES: &[&str] = &[
    "updated", "updatedat", "modified", "modifiedat", "lastmodified", "更新时间", "修改时间",
];

/// 表头/字段名 → 规范字段。未知字段返回 `None`(直接忽略,例如 1Password 的 `type`)。
fn field_from_key(raw: &str) -> Option<Field> {
    let key = normalize_key(raw);
    if key.is_empty() {
        return None;
    }
    let table: &[(&[&str], Field)] = &[
        (ID_ALIASES, Field::Id),
        (TITLE_ALIASES, Field::Title),
        (USERNAME_ALIASES, Field::Username),
        (PASSWORD_ALIASES, Field::Password),
        (URL_ALIASES, Field::Url),
        (API_KEY_ALIASES, Field::ApiKey),
        (API_ENDPOINT_ALIASES, Field::ApiEndpoint),
        (CATEGORY_ALIASES, Field::Category),
        (TAGS_ALIASES, Field::Tags),
        (FAVORITE_ALIASES, Field::Favorite),
        (NOTES_ALIASES, Field::Notes),
        (CREATED_ALIASES, Field::Created),
        (UPDATED_ALIASES, Field::Updated),
    ];
    for (aliases, field) in table {
        if aliases.contains(&key.as_str()) {
            return Some(*field);
        }
    }
    None
}

/// 归一化字段名:去空白与常见分隔符、统一小写、去掉 BOM。
fn normalize_key(raw: &str) -> String {
    raw.trim()
        .trim_start_matches('\u{feff}')
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, '_' | '-' | '.' | '(' | ')' | ':' | '/'))
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn parse_csv(text: &str) -> Result<Vec<Entry>> {
    let records = parse_csv_records(text)?;
    let header_index = records
        .iter()
        .position(|r| r.iter().any(|c| !c.trim().is_empty()))
        .ok_or_else(|| VaultError::Invalid("文件是空的。".into()))?;

    let header: Vec<Option<Field>> = records[header_index].iter().map(|h| field_from_key(h)).collect();

    if !header
        .iter()
        .any(|f| matches!(f, Some(Field::Title) | Some(Field::Username) | Some(Field::Password)))
    {
        return Err(VaultError::Invalid(
            "认不出表头。第一行要有标题、用户名或密码等列名。".into(),
        ));
    }

    // 只有本程序导出的文件才可能带公式防护前缀,见 is_own_export。
    let unguard = is_own_export(text, &records[header_index]);

    let mut out = Vec::new();
    for record in &records[header_index + 1..] {
        if record.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let entry = entry_from_row(&header, record, unguard);
        if is_blank_entry(&entry) {
            continue;
        }
        if out.len() == MAX_IMPORT_ENTRIES {
            // 只往后看这一个真实条目就收手,不再为剩下的行分配 Entry。
            return Err(too_many_entries(out.len() + 1));
        }
        out.push(entry);
    }

    if out.is_empty() {
        return Err(VaultError::Invalid("表头下面没有数据。".into()));
    }
    Ok(out)
}

/// 这份 CSV 是不是本程序自己导出的(也就是:里面的单元格有没有可能被加了公式防护前缀)。
///
/// 判据是「带 BOM + 表头正好是我们那几列、顺序一致」。必须这么判而不能无条件去前缀,
/// 因为防护前缀是一个**带内**转义(单引号 + 原值),而单引号本身完全可以是数据的一部分:
/// 别人家的导出里 `'=x` 就是一个以单引号开头的真实密码,无条件去前缀会把它吃掉一个字符,
/// 覆盖导入时就成了「静默改写用户密码」。
///
/// 0.3.0 起表头从 7 列变 9 列,0.2.x 导出的旧文件必须继续被认出来 ——
/// 否则升级后第一次导入自家旧文件,防护前缀就不再还原(值凭空多一个单引号)。
/// 所以对新旧两代表头都做匹配。
///
/// 代价说清楚:如果一个文件既带 BOM、表头又和我们的(任一代表头)完全一致,就会被当成
/// 自家文件去前缀。这种文件实际上就是我们的格式家族(本程序、以及本程序导出后用
/// Excel「CSV UTF-8」另存的),对它们去前缀是必需的 —— 否则 Excel 往返一次就会多出单引号。
fn is_own_export(text: &str, raw_header: &[String]) -> bool {
    if !text.starts_with('\u{feff}') {
        return false;
    }

    // 0.2.x 的表头(还没有 API 密钥/端点)。
    const LEGACY_HEADER: [&str; 7] =
        ["Title", "Username", "Password", "Url", "Category", "Tags", "Notes"];

    // Password / ApiKey 两列随导出选项成对省略,是否出现由实际表头决定;
    // 两个判定彼此独立(不要求同时出现)—— 手工在 Excel 里删掉其中一列的文件
    // 仍按自家处理,防护前缀照常还原,否则值会静默多出一个单引号。
    let has_password = raw_header.iter().any(|h| normalize_key(h) == "password");
    let has_api_key = raw_header.iter().any(|h| normalize_key(h) == "apikey");

    [CSV_HEADER.as_slice(), LEGACY_HEADER.as_slice()]
        .iter()
        .any(|columns| {
            let expected: Vec<&str> = columns
                .iter()
                .copied()
                .filter(|column| match *column {
                    "Password" => has_password,
                    "ApiKey" => has_api_key,
                    _ => true,
                })
                .collect();
            raw_header.len() == expected.len()
                && raw_header
                    .iter()
                    .zip(expected)
                    .all(|(got, want)| normalize_key(got) == normalize_key(want))
        })
}

/// 一个字段的原始值:CSV 是单元格文本,JSON 还可能是布尔 / 数字 / 数组(标签)。
enum FieldValue<'a> {
    Text(String),
    Bool(bool),
    Number(&'a serde_json::Number),
    List(&'a [Value]),
}

impl FieldValue<'_> {
    /// 归一成文本(数组与空值给出空串,与旧的 JSON 解析行为一致)。
    fn text(self) -> String {
        match self {
            FieldValue::Text(s) => s,
            FieldValue::Bool(b) => b.to_string(),
            FieldValue::Number(n) => n.to_string(),
            FieldValue::List(_) => String::new(),
        }
    }

    /// 真值:布尔与数字直接判定,其余按文本(兼容 `1`/`yes`/`是` 等写法)。
    /// 认不出来的一律当「否」——收藏不是关键数据,宁可漏掉也不要误判。
    fn truthy(self) -> bool {
        match self {
            FieldValue::Bool(b) => b,
            FieldValue::Number(n) => n.as_i64().is_some_and(|i| i != 0),
            other => truthy(&other.text()),
        }
    }
}

/// 把一个字段装进条目。CSV 与 JSON 两个入口把原始值归一成 [`FieldValue`] 后走这里,
/// 字段语义只在这一处定义,不会两边各写一份再慢慢走样。
fn apply_field(entry: &mut Entry, field: Field, value: FieldValue<'_>) {
    match field {
        Field::Id => entry.id = value.text().trim().to_string(),
        Field::Title => entry.title = value.text(),
        Field::Username => entry.username = value.text(),
        Field::Password => entry.password = Zeroizing::new(value.text()),
        Field::Url => entry.url = value.text(),
        Field::ApiKey => entry.api_key = Zeroizing::new(value.text()),
        Field::ApiEndpoint => entry.api_endpoint = value.text(),
        Field::Category => entry.category = value.text(),
        Field::Tags => match value {
            FieldValue::List(items) => {
                for item in items {
                    push_tags(&mut entry.tags, &as_string(item));
                }
            }
            other => push_tags(&mut entry.tags, &other.text()),
        },
        Field::Favorite => entry.favorite = value.truthy(),
        Field::Notes => entry.notes = value.text(),
        Field::Created => entry.created = parse_timestamp(&value.text()),
        Field::Updated => entry.updated = parse_timestamp(&value.text()),
    }
}

/// 拆分一段标签文本,去重地并入列表(跨数组元素、跨同名列之间都去重)。
fn push_tags(out: &mut Vec<String>, value: &str) {
    for tag in split_tags(value) {
        if !out.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
            out.push(tag);
        }
    }
}

fn entry_from_row(header: &[Option<Field>], record: &[String], unguard: bool) -> Entry {
    let mut entry = Entry::default();

    for (index, field) in header.iter().enumerate() {
        let Some(field) = field else { continue };
        let raw = record.get(index).map(String::as_str).unwrap_or("");
        // 只有自家导出的文件才去前缀:第三方文件里的单引号是数据本身。
        let value = if unguard {
            unguard_formula(raw)
        } else {
            raw.to_string()
        };
        apply_field(&mut entry, *field, FieldValue::Text(value));
    }

    entry
}

/// 标签字符串 → 标签列表(去空、去重,保留首次出现的写法)。
pub fn split_tags(value: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in value.split([TAG_SEPARATOR, '|', '\n']) {
        let tag = part.trim();
        if tag.is_empty() {
            continue;
        }
        if !out.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
            out.push(tag.to_string());
        }
    }
    out
}

fn parse_timestamp(value: &str) -> i64 {
    let value = value.trim();
    if value.is_empty() {
        return 0;
    }
    // 导出的是 Unix 秒;也容忍 ISO-8601 里的纯数字前缀之外的情况(直接放弃)。
    value.parse::<i64>().unwrap_or(0).max(0)
}

/// 一条记录是不是「什么都没填」——导入时用来丢掉 CSV 里的空行与占位行。
///
/// `pub(crate)`:供 `vault::import_entries` 复用判定,不属于对外 API。
pub(crate) fn is_blank_entry(entry: &Entry) -> bool {
    entry.title.trim().is_empty()
        && entry.username.trim().is_empty()
        && entry.password.is_empty()
        && entry.url.trim().is_empty()
        && entry.api_key.is_empty()
        && entry.api_endpoint.trim().is_empty()
        && entry.notes.trim().is_empty()
}

/// RFC 4180 解析器,额外容忍 BOM、CRLF/CR/LF 与字段内换行。
///
/// 返回 `Err` 而不是无限膨胀:这个函数先把**整个文件**展开成 `Vec<Vec<String>>`,
/// 那是导入链路上最占内存的一步(之后才逐行转成 `Entry`)。正文行数在这里就卡住,
/// 于是即使调用方([`parse`])拿到的文本已经绕过 [`import_from_path`] 的体积检查,
/// 也不会被一个几百万行的 CSV 拖垮。表头不计入上限。
fn parse_csv_records(text: &str) -> Result<Vec<Vec<String>>> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    // 已展开的行数,含表头与空行 —— 空行也占预算,理由见下面的注释。
    let mut rows = 0usize;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();

    let take_row =
        |records: &mut Vec<Vec<String>>, record: &mut Vec<String>, rows: &mut usize| -> Result<()> {
            *rows += 1;
            // 第 1 行当表头不计,所以实际放行的是「上限 + 表头」这么多行。
            if *rows > MAX_IMPORT_ENTRIES + 1 {
                return Err(too_many_entries(*rows - 1));
            }
            records.push(std::mem::take(record));
            Ok(())
        };

    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(c);
            }
            continue;
        }

        match c {
            '"' => in_quotes = true,
            ',' => record.push(std::mem::take(&mut field)),
            '\r' | '\n' => {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                record.push(std::mem::take(&mut field));
                // 空行同样占预算:一个 64 MB 的纯换行文件能撑出六千多万个
                // Vec<String>,而它们最终一条条目都变不出来。若只统计非空行,
                // 这档输入会被原样放过去 —— 体积上限挡不住它(csv_row_limit_
                // counts_blank_lines_too 那条测试正是这个场景)。
                take_row(&mut records, &mut record, &mut rows)?;
            }
            _ => field.push(c),
        }
    }

    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        take_row(&mut records, &mut record, &mut rows)?;
    }
    Ok(records)
}

fn parse_json(text: &str) -> Result<Vec<Entry>> {
    let value: Value = serde_json::from_str(text)?;

    let items: &Vec<Value> = match &value {
        Value::Array(items) => items,
        Value::Object(map) => match map.get("entries") {
            Some(Value::Array(items)) => items,
            _ => {
                return Err(VaultError::Invalid(
                    "JSON 里找不到 entries 数组（可先导出为 CSV）。".into(),
                ));
            }
        },
        _ => {
            return Err(VaultError::Invalid(
                "JSON 顶层不是对象也不是数组。".into(),
            ));
        }
    };

    let mut out = Vec::new();
    for item in items {
        let Value::Object(map) = item else { continue };
        let entry = entry_from_object(map);
        if is_blank_entry(&entry) {
            continue;
        }
        if out.len() == MAX_IMPORT_ENTRIES {
            return Err(too_many_entries(out.len() + 1));
        }
        out.push(entry);
    }

    if out.is_empty() {
        return Err(VaultError::Invalid("JSON 里没有可导入的条目。".into()));
    }
    Ok(out)
}

/// JSON 值 → [`FieldValue`]:布尔 / 数字 / 数组各有形态,其余按文本处理。
fn json_field_value(value: &Value) -> FieldValue<'_> {
    match value {
        Value::Bool(b) => FieldValue::Bool(*b),
        Value::Number(n) => FieldValue::Number(n),
        Value::Array(items) => FieldValue::List(items),
        other => FieldValue::Text(as_string(other)),
    }
}

fn entry_from_object(map: &Map<String, Value>) -> Entry {
    let mut entry = Entry::default();

    for (key, value) in map {
        let Some(field) = field_from_key(key) else { continue };
        apply_field(&mut entry, field, json_field_value(value));
    }

    entry
}

/// 文本形式的真值(CSV 单元格与 JSON 字符串共用)。
fn truthy(text: &str) -> bool {
    matches!(
        text.trim().to_lowercase().as_str(),
        "1" | "true" | "yes" | "y" | "on" | "是" | "真" | "收藏"
    )
}

fn as_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        _ => String::new(),
    }
}

/// 去重键:标题 + 用户名(忽略大小写与首尾空白);两者都空时退化为网址。
///
/// 返回 `None` 表示这条记录没有可用来比对的标识,永远算「不重复」。
/// `pub(crate)`:导入去重的实现细节,由 `vault::import_entries` 使用。
pub(crate) fn dedupe_key(entry: &Entry) -> Option<String> {
    let title = lower(&entry.title);
    let username = lower(&entry.username);
    if !title.is_empty() || !username.is_empty() {
        return Some(format!("t:{title}\u{1}u:{username}"));
    }
    let url = lower(&entry.url);
    if url.is_empty() {
        None
    } else {
        Some(format!("u:{url}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 导出/导入往返的期望值:(标题, 密码, 网址, 分类, 备注, 标签, API 密钥, API 端点)。
    type ExpectedRow = (String, String, String, String, String, String, String, String);

    fn sample_document() -> Document {
        Document {
            categories: vec!["工作".into()],
            tags: vec!["重要".into()],
            entries: vec![
                Entry {
                    title: "带,逗号".into(),
                    username: "a\"b".into(),
                    password: Zeroizing::new("p,w\"d\n换行".into()),
                    url: "https://example.com".into(),
                    api_key: Zeroizing::new("sk-abc".into()),
                    api_endpoint: "https://api.example.com/v1".into(),
                    category: "工作".into(),
                    tags: vec!["重要".into(), "内部".into()],
                    notes: "第一行\n第二行".into(),
                    created: 100,
                    updated: 200,
                    ..Default::default()
                },
                Entry {
                    title: "另一个".into(),
                    username: "user".into(),
                    password: Zeroizing::new("secret".into()),
                    ..Default::default()
                },
                Entry {
                    title: "在回收站里".into(),
                    deleted: Some(1),
                    ..Default::default()
                },
            ],
            ..Default::default()
        }
    }

    #[test]
    fn csv_round_trips_all_fields() {
        let document = sample_document();
        let text = export_csv(&document, ExportOptions::default());
        let parsed = parse_csv(&text).unwrap();

        assert_eq!(parsed.len(), 2, "回收站条目不应被导出");
        assert_eq!(parsed[0].title, "另一个");
        assert_eq!(parsed[1].title, "带,逗号");
        assert_eq!(parsed[1].username, "a\"b");
        assert_eq!(parsed[1].password.as_str(), "p,w\"d\n换行");
        assert_eq!(parsed[1].api_key.as_str(), "sk-abc");
        assert_eq!(parsed[1].api_endpoint, "https://api.example.com/v1");
        assert_eq!(parsed[1].tags, vec!["重要".to_string(), "内部".to_string()]);
        assert_eq!(parsed[1].notes, "第一行\n第二行");
    }

    #[test]
    fn csv_has_bom_and_crlf() {
        let text = export_csv(&sample_document(), ExportOptions::default());
        assert!(text.starts_with('\u{feff}'));
        assert!(text.contains("\r\n"));
        assert!(text.starts_with(
            "\u{feff}Title,Username,Password,Url,ApiKey,ApiEndpoint,Category,Tags,Notes"
        ));
    }

    #[test]
    fn csv_omits_secret_columns_when_disabled() {
        let document = sample_document();
        let text = export_csv(
            &document,
            ExportOptions {
                include_secrets: false,
            },
        );
        assert!(!text.contains("secret"), "密码不该写出");
        assert!(!text.contains("sk-abc"), "API 密钥不该写出");
        assert!(text.starts_with(
            "\u{feff}Title,Username,Url,ApiEndpoint,Category,Tags,Notes"
        ));

        // 数据行必须与 7 列表头逐列对齐:去掉两列后最容易错位的就是这里。
        let parsed = parse_csv(&text).unwrap();
        let entry = parsed.iter().find(|e| e.title == "带,逗号").unwrap();
        assert_eq!(entry.username, "a\"b");
        assert_eq!(entry.url, "https://example.com");
        assert_eq!(entry.api_endpoint, "https://api.example.com/v1");
        assert_eq!(entry.category, "工作");
        assert_eq!(entry.tags, vec!["重要".to_string(), "内部".to_string()]);
        assert_eq!(entry.notes, "第一行\n第二行");
        assert!(entry.password.is_empty());
        assert!(entry.api_key.is_empty());
    }

    #[test]
    fn formula_prefixes_are_guarded_and_listed() {
        let document = Document {
            entries: vec![Entry {
                title: "=cmd|'/C calc'!A1".into(),
                username: "@SUM(1)".into(),
                password: Zeroizing::new("-2+3".into()),
                ..Default::default()
            }],
            ..Default::default()
        };

        let text = export_csv(&document, ExportOptions::default());
        assert!(text.contains("'=cmd"));
        assert!(text.contains("'@SUM"));

        let parsed = parse_csv(&text).unwrap();
        assert_eq!(parsed[0].title, "=cmd|'/C calc'!A1");
        assert_eq!(parsed[0].username, "@SUM(1)");
        assert_eq!(parsed[0].password.as_str(), "-2+3");
    }

    #[test]
    fn leading_apostrophe_round_trips() {
        // 单引号本身是防护前缀,必须能被转义/还原,否则「密码以 '= 开头」会掉字符。
        let values = ["'普通单引号", "'=SUM(1)", "'", "''", "''''=x"];
        let mut document = Document::default();
        for (index, value) in values.iter().enumerate() {
            document.entries.push(Entry {
                title: format!("{index:02}"),
                username: (*value).to_string(),
                password: Zeroizing::new((*value).to_string()),
                ..Default::default()
            });
        }

        let text = export_csv(&document, ExportOptions::default());
        let parsed = parse_csv(&text).unwrap();
        for (entry, value) in parsed.iter().zip(values.iter()) {
            assert_eq!(&entry.username, value);
            assert_eq!(entry.password.as_str(), *value);
        }
    }

    /// 第三方 CSV 里的单引号是数据本身,不能被当成我们的防护前缀吃掉。
    /// (实测过:无条件去前缀会把 `'=x` 变成 `=x`、`''e` 变成 `'e` —— 静默改写密码。)
    #[test]
    fn foreign_csv_keeps_leading_apostrophes() {
        let text = "name,url,username,password,note\n\
                    A,https://a.example,alice,'=x,\n\
                    B,https://b.example,bob,''e,\n\
                    C,https://c.example,carol,'-abc,\n\
                    D,https://d.example,dave,'@home,\n";
        let parsed = parse_csv(text).unwrap();
        let passwords: Vec<&str> = parsed.iter().map(|e| e.password.as_str()).collect();
        assert_eq!(passwords, vec!["'=x", "''e", "'-abc", "'@home"]);
    }

    /// 判据要求「带 BOM + 表头就是我们的那几列」。手搓一份同样表头但不带 BOM 的文件,
    /// 里面的单引号按数据保留;同一份内容带上 BOM(即本程序导出的形态)才还原前缀。
    #[test]
    fn only_own_export_gets_unguarded() {
        let text = "Title,Username,Password,Url,Category,Tags,Notes\r\n\
                    T,u,'=keep,https://x.example,工作,,\r\n";
        let foreign = parse_csv(text).unwrap();
        assert_eq!(foreign[0].password.as_str(), "'=keep", "无 BOM 不算自家文件");
        assert_eq!(foreign[0].username, "u");

        let own = parse_csv(&format!("\u{feff}{text}")).unwrap();
        assert_eq!(own[0].password.as_str(), "=keep", "自家文件要还原前缀");
    }

    /// 0.2.x 的 7 列表头、含/不含密码两形态,升级后都必须继续被当自家文件。
    #[test]
    fn legacy_seven_column_headers_are_still_recognized() {
        let with_pw = "\u{feff}Title,Username,Password,Url,Category,Tags,Notes\r\n\
                       T,u,'=keep,https://x.example,工作,,\r\n";
        let parsed = parse_csv(with_pw).unwrap();
        assert_eq!(parsed[0].password.as_str(), "=keep");

        let without_pw = "\u{feff}Title,Username,Url,Category,Tags,Notes\r\n\
                          T,'=user,https://x.example,工作,,\r\n";
        let parsed = parse_csv(without_pw).unwrap();
        assert_eq!(parsed[0].username, "=user", "旧版不含密码的导出也要还原前缀");
    }

    /// 0.3.0 的 9 列表头、含/不含凭据两形态,同样算自家文件。
    #[test]
    fn new_nine_column_headers_are_recognized() {
        let with_secrets = "\u{feff}Title,Username,Password,Url,ApiKey,ApiEndpoint,Category,Tags,Notes\r\n\
                            T,u,'=keep,https://x.example,'=key,https://api.x.example,工作,,\r\n";
        let parsed = parse_csv(with_secrets).unwrap();
        assert_eq!(parsed[0].password.as_str(), "=keep");
        assert_eq!(parsed[0].api_key.as_str(), "=key");
        assert_eq!(parsed[0].api_endpoint, "https://api.x.example");

        let without_secrets = "\u{feff}Title,Username,Url,ApiEndpoint,Category,Tags,Notes\r\n\
                               T,'=user,https://api.y.example,工作,,\r\n";
        let parsed = parse_csv(without_secrets).unwrap();
        assert_eq!(parsed[0].username, "=user", "不含凭据的 9 列导出也要还原前缀");
    }

    /// 两个可选列各自独立判定:手工删掉其中一列的文件仍按自家处理,
    /// 前缀必须还原(否则值会静默多出一个单引号)。这是有意的宽松。
    #[test]
    fn mixed_credential_headers_are_treated_as_own() {
        let no_password = "\u{feff}Title,Username,Url,ApiKey,ApiEndpoint,Category,Tags,Notes\r\n\
                           T,'=user,https://x.example,'=key,https://api.x.example,工作,,\r\n";
        let parsed = parse_csv(no_password).unwrap();
        assert_eq!(parsed[0].username, "=user");
        assert_eq!(parsed[0].api_key.as_str(), "=key");

        let no_key = "\u{feff}Title,Username,Password,Url,ApiEndpoint,Category,Tags,Notes\r\n\
                      T,u,'=pw,https://x.example,https://api.x.example,工作,,\r\n";
        let parsed = parse_csv(no_key).unwrap();
        assert_eq!(parsed[0].password.as_str(), "=pw");
    }

    /// 新字段在 CSV / JSON 两侧都能按别名识别。
    #[test]
    fn recognizes_api_key_and_endpoint_columns() {
        let csv = "Title,Username,Password,Url,API Key,API Endpoint,Notes\r\n\
                   T,u,pw,https://x.example,sk-1,https://api.x.example/v1,n\r\n";
        let parsed = parse_csv(csv).unwrap();
        assert_eq!(parsed[0].api_key.as_str(), "sk-1");
        assert_eq!(parsed[0].api_endpoint, "https://api.x.example/v1");

        let json = r#"[{"title":"T","api_key":"sk-2","apiEndpoint":"https://api.y.example"}]"#;
        let parsed = parse_json(json).unwrap();
        assert_eq!(parsed[0].api_key.as_str(), "sk-2");
        assert_eq!(parsed[0].api_endpoint, "https://api.y.example");
    }

    /// 只有 API 密钥/端点的行不算空行 —— 它们带着真实数据,不该被丢弃。
    #[test]
    fn entry_with_only_api_fields_is_not_blank() {
        let entry = Entry {
            api_key: Zeroizing::new("sk-only".into()),
            ..Default::default()
        };
        assert!(!is_blank_entry(&entry));

        let parsed = parse_json(r#"[{"title":"","api_key":"sk-only"}]"#).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].api_key.as_str(), "sk-only");
    }

    /// `parse()` 不能先把 BOM 吃掉再交给 CSV 解析器 —— 那会把自家导出当外人,
    /// 于是 Excel 往返一次就多出一堆单引号。
    #[test]
    fn parse_keeps_the_bom_for_the_own_export_check() {
        let mut document = Document::default();
        document.entries.push(Entry {
            title: "表单里手输的".into(),
            username: "'=user".into(),
            password: Zeroizing::new("'=pw".into()),
            ..Default::default()
        });

        let text = export_csv(&document, ExportOptions::default());
        let via_parse = parse(&text, None).unwrap();
        assert_eq!(via_parse[0].username, "'=user");
        assert_eq!(via_parse[0].password.as_str(), "'=pw");
    }

    /// 各种「会破坏 CSV 结构」的取值必须原样往返。
    #[test]
    fn csv_survives_nasty_fields() {
        const NASTY: [&str; 20] = [
            "",
            " ",
            " 前后空格 ",
            "a,b",
            "a\"b",
            "a\"\"b",
            "换行\n内",
            "回车\r\n内",
            "=1+1",
            "+1",
            "-1",
            "@x",
            "'=x",
            ";",
            "|",
            "a;b",
            "逗号,引号\"换行\n",
            "\\",
            "-",
            "'",
        ];

        let mut document = Document::default();
        for (index, value) in NASTY.iter().enumerate() {
            document.entries.push(Entry {
                title: format!("{index:02}"),
                username: (*value).to_string(),
                password: Zeroizing::new((*value).to_string()),
                url: (*value).to_string(),
                api_key: Zeroizing::new((*value).to_string()),
                api_endpoint: (*value).to_string(),
                category: (*value).to_string(),
                tags: vec![(*value).to_string()],
                notes: (*value).to_string(),
                ..Default::default()
            });
        }

        let text = export_csv(&document, ExportOptions::default());
        let parsed = parse_csv(&text).unwrap();
        assert_eq!(parsed.len(), NASTY.len());

        for (entry, value) in parsed.iter().zip(NASTY.iter()) {
            assert_eq!(&entry.username, value, "用户名往返失败:{value:?}");
            assert_eq!(entry.password.as_str(), *value, "密码往返失败:{value:?}");
            assert_eq!(entry.api_key.as_str(), *value, "API 密钥往返失败:{value:?}");
            assert_eq!(&entry.api_endpoint, value, "API 端点往返失败:{value:?}");
            assert_eq!(&entry.url, value, "网址往返失败:{value:?}");
            assert_eq!(&entry.category, value, "分类往返失败:{value:?}");
            assert_eq!(&entry.notes, value, "备注往返失败:{value:?}");
            assert_eq!(entry.tags, split_tags(value), "标签往返失败:{value:?}");
        }
    }

    /// 确定性伪随机(不引入依赖)地生成大量刁钻字段,验证 CSV 往返恒等。
    ///
    /// 手写用例只能覆盖想到的情况;这个测试用固定种子的 xorshift
    /// 把标点、空白、中文与所有「防护字符」随机组合起来,覆盖面大得多,
    /// 而且失败时可以复现。
    #[test]
    fn csv_round_trips_random_fields() {
        const ALPHABET: [char; 20] = [
            'a', 'Z', '0', ',', '"', '\n', '\r', ' ', '\t', ';', '|', '=', '+', '-', '@', '\'', '中', '文',
            '\\', '\u{feff}',
        ];

        let mut seed: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let mut random_field = || {
            let len = (next() % 12) as usize;
            (0..len)
                .map(|_| ALPHABET[(next() % ALPHABET.len() as u64) as usize])
                .collect::<String>()
        };

        let mut document = Document::default();
        let mut expected: Vec<ExpectedRow> = Vec::new();
        for index in 0..300 {
            let title = random_field();
            let password = random_field();
            let url = random_field();
            let category = random_field();
            let notes = random_field();
            let tags = random_field();
            let api_key = random_field();
            let api_endpoint = random_field();
            expected.push((
                title.clone(),
                password.clone(),
                url.clone(),
                category.clone(),
                notes.clone(),
                tags.clone(),
                api_key.clone(),
                api_endpoint.clone(),
            ));
            document.entries.push(Entry {
                // 用户名固定非空:否则整条全空的行会在解析时被当作空行丢掉。
                title,
                username: format!("user-{index}"),
                password: Zeroizing::new(password),
                url,
                api_key: Zeroizing::new(api_key),
                api_endpoint,
                category,
                tags: vec![tags],
                notes,
                ..Default::default()
            });
        }

        let text = export_csv(&document, ExportOptions::default());
        let parsed = parse_csv(&text).unwrap();
        assert_eq!(parsed.len(), expected.len(), "行数应保持不变");

        // 导出会按标题排序,所以按「标题 + 用户名」回查,而不是按位置对齐。
        for entry in &parsed {
            let index: usize = entry
                .username
                .strip_prefix("user-")
                .and_then(|s| s.parse().ok())
                .expect("用户名应原样保留");
            let want = &expected[index];

            assert_eq!(entry.title, want.0, "标题往返失败:{:?}", want.0);
            assert_eq!(entry.password.as_str(), want.1, "密码往返失败:{:?}", want.1);
            assert_eq!(entry.api_key.as_str(), want.6, "API 密钥往返失败:{:?}", want.6);
            assert_eq!(entry.api_endpoint, want.7, "API 端点往返失败:{:?}", want.7);
            assert_eq!(entry.url, want.2, "网址往返失败:{:?}", want.2);
            assert_eq!(entry.category, want.3, "分类往返失败:{:?}", want.3);
            assert_eq!(entry.notes, want.4, "备注往返失败:{:?}", want.4);
            assert_eq!(entry.tags, split_tags(&want.5), "标签往返失败:{:?}", want.5);
        }
    }

    #[test]
    fn recognizes_third_party_headers() {
        let chrome = "name,url,username,password,note\n示例,https://a.com,me,pw,备注\n";
        let parsed = parse_csv(chrome).unwrap();
        assert_eq!(parsed[0].title, "示例");
        assert_eq!(parsed[0].url, "https://a.com");
        assert_eq!(parsed[0].password.as_str(), "pw");
        assert_eq!(parsed[0].notes, "备注");

        let keepass = "Account,Login Name,Password,Web Site,Comments\nK,user,pw,https://k.example,note\n";
        let parsed = parse_csv(keepass).unwrap();
        assert_eq!(parsed[0].title, "K");
        assert_eq!(parsed[0].username, "user");
        assert_eq!(parsed[0].url, "https://k.example");

        let bitwarden = "folder,favorite,type,name,notes,login_uri,login_username,login_password\n工作,1,login,条目,note,https://b.example,u,p\n";
        let parsed = parse_csv(bitwarden).unwrap();
        assert_eq!(parsed[0].category, "工作");
        assert_eq!(parsed[0].title, "条目");
        assert_eq!(parsed[0].username, "u");
        assert!(parsed[0].favorite, "Bitwarden 的 favorite 列");

        // LastPass 的列名与别家都不一样,收藏列叫 `fav`(README 点名支持它)。
        let lastpass = "url,username,password,extra,name,grouping,fav\n\
                        https://l.example,me,pw,note,条目,工作,1\n";
        let parsed = parse_csv(lastpass).unwrap();
        assert_eq!(parsed[0].url, "https://l.example");
        assert_eq!(parsed[0].username, "me");
        assert_eq!(parsed[0].password.as_str(), "pw");
        assert_eq!(parsed[0].notes, "note");
        assert_eq!(parsed[0].title, "条目");
        assert_eq!(parsed[0].category, "工作");
        assert!(parsed[0].favorite, "LastPass 的 fav 列");
    }

    #[test]
    fn tolerates_bom_blank_lines_and_short_rows() {
        let text = "\u{feff}标题,用户名,密码\r\n\r\n只有标题\r\n,只有用户,\r\n";
        let parsed = parse_csv(text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[0].title, "只有标题");
        assert_eq!(parsed[1].username, "只有用户");
    }

    #[test]
    fn rejects_unrecognized_header() {
        let err = parse_csv("foo,bar\n1,2\n").unwrap_err();
        assert!(matches!(err, VaultError::Invalid(_)));
    }

    #[test]
    fn json_round_trips_with_taxonomy_and_timestamps() {
        let document = sample_document();
        let text = export_json(&document, ExportOptions::default()).unwrap();
        assert!(text.contains(JSON_FORMAT_TAG));

        let parsed = parse_json(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert_eq!(parsed[1].title, "带,逗号");
        assert_eq!(parsed[1].created, 100);
        assert_eq!(parsed[1].tags, vec!["重要".to_string(), "内部".to_string()]);
    }

    #[test]
    fn json_round_trips_favorite() {
        let mut document = sample_document();
        document.entries[0].favorite = true;
        let text = export_json(&document, ExportOptions::default()).unwrap();
        assert!(text.contains("\"favorite\": true"), "JSON 应显式写出收藏状态");

        let parsed = parse_json(&text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed[1].favorite, "收藏应随 JSON 往返");
        assert!(!parsed[0].favorite);
    }

    #[test]
    fn json_round_trips_api_fields() {
        let document = sample_document();
        let text = export_json(&document, ExportOptions::default()).unwrap();
        let parsed = parse_json(&text).unwrap();
        let entry = parsed.iter().find(|e| e.title == "带,逗号").unwrap();
        assert_eq!(entry.api_key.as_str(), "sk-abc");
        assert_eq!(entry.api_endpoint, "https://api.example.com/v1");
    }

    #[test]
    fn json_omits_api_key_when_secrets_disabled() {
        let document = sample_document();
        let text = export_json(
            &document,
            ExportOptions {
                include_secrets: false,
            },
        )
        .unwrap();
        assert!(!text.contains("sk-abc"), "关闭明文选项后不该写出 API 密钥");
        assert!(
            text.contains("https://api.example.com/v1"),
            "端点与网址同级,照常导出"
        );
        let parsed = parse_json(&text).unwrap();
        let entry = parsed.iter().find(|e| e.title == "带,逗号").unwrap();
        assert!(entry.api_key.is_empty());
    }

    #[test]
    fn json_accepts_string_and_numeric_favorite() {
        let text = r#"{"entries":[{"title":"A","favorite":"yes"},{"title":"B","favorite":0}]}"#;
        let parsed = parse_json(text).unwrap();
        assert!(parsed[0].favorite);
        assert!(!parsed[1].favorite);
    }

    #[test]
    fn bitwarden_favorite_column_is_recognized() {
        let text = "folder,favorite,type,name,notes,login_uri,login_username,login_password\n\
                    工作,1,login,GitHub,,https://github.com,alice,pw\n\
                    个人,0,login,邮箱,,,bob,pw2\n";
        let parsed = parse_csv(text).unwrap();
        assert_eq!(parsed.len(), 2);
        assert!(parsed[0].favorite, "Bitwarden 的 favorite=1 应识别为收藏");
        assert!(!parsed[1].favorite);
        assert_eq!(parsed[0].password.as_str(), "pw");
    }

    #[test]
    fn csv_export_has_no_favorite_column() {
        let mut document = sample_document();
        document.entries[0].favorite = true;
        let text = export_csv(&document, ExportOptions::default());
        assert!(!text.to_lowercase().contains("favorite"), "CSV 表头固定 9 列,不含收藏");
        let parsed = parse_csv(&text).unwrap();
        assert!(parsed.iter().all(|e| !e.favorite), "CSV 不承载收藏");
    }

    #[test]
    fn json_accepts_bare_array_and_aliases() {
        let text = r#"[{"name":"标题","login":"用户","password":"p","tags":"a|b"}]"#;
        let parsed = parse_json(text).unwrap();
        assert_eq!(parsed[0].title, "标题");
        assert_eq!(parsed[0].username, "用户");
        assert_eq!(parsed[0].tags, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn json_import_ignores_unknown_keys() {
        let text = r#"{"format":"password-notebook","version":1,"entries":[{"title":"T","totp":"x","fields":[{"name":"pin"}]}]}"#;
        let parsed = parse_json(text).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].title, "T");
    }

    #[test]
    fn detect_format_by_first_character() {
        assert_eq!(detect_format("{\"entries\":[]}"), Format::Json);
        assert_eq!(detect_format("  [ {\"title\":\"a\"} ]"), Format::Json);
        assert_eq!(detect_format("\u{feff}Title,Username\n"), Format::Csv);
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(parse("   ", None).is_err());
        assert!(parse("", None).is_err());
    }

    #[test]
    fn dedupe_key_ignores_case_and_falls_back_to_url() {
        let a = Entry {
            title: " GitHub ".into(),
            username: "Me".into(),
            ..Default::default()
        };
        let b = Entry {
            title: "github".into(),
            username: "me".into(),
            ..Default::default()
        };
        assert_eq!(dedupe_key(&a), dedupe_key(&b));

        let c = Entry {
            url: "https://Only-Url.example".into(),
            ..Default::default()
        };
        let d = Entry {
            url: "https://only-url.example".into(),
            ..Default::default()
        };
        assert_eq!(dedupe_key(&c), dedupe_key(&d));

        assert_eq!(dedupe_key(&Entry::default()), None);
    }

    #[test]
    fn csv_parser_handles_embedded_newlines_and_quotes() {
        let text = "Title,Notes\n\"多行\",\"第一行\n第二行\"\n\"引号\"\"内部\",x\n";
        let records = parse_csv_records(text).unwrap();
        assert_eq!(records.len(), 3);
        assert_eq!(records[1][1], "第一行\n第二行");
        assert_eq!(records[2][0], "引号\"内部");
    }

    #[test]
    fn human_size_reads_as_human_units() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1024), "1 KB");
        assert_eq!(human_size(MAX_IMPORT_BYTES), "64 MB");
        // 一位小数与「平局取偶」的取整口径(与旧浮点写法逐项一致)。
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(1280), "1.2 KB");
        assert_eq!(human_size(1_048_575), "1024 KB");
    }

    /// 空行也要占条数预算 —— 否则一整个 64 MB 的纯换行文件能撑出六千多万个
    /// `Vec<String>`,而体积上限刚好放它过关(六千多万条空行变不出任何条目)。
    #[test]
    fn csv_row_limit_counts_blank_lines_too() {
        let mut text = String::from("Title,Username\n");
        for _ in 0..(MAX_IMPORT_ENTRIES + 10) {
            text.push('\n');
        }
        text.push_str("A,a\n");
        let err = parse(&text, Some(Format::Csv)).unwrap_err();
        assert!(
            err.to_string().contains("条目太多"),
            "空行也必须计入上限,否则空行绕过预算:{err}"
        );
    }

    /// 上限内的空行仍然是容忍的 —— 上限是「超过就拒绝」,不是「见到空行就拒绝」。
    #[test]
    fn csv_tolerates_blank_lines_under_the_limit() {
        let text = "Title,Username\n\nA,a\n\n\nB,b\n";
        let entries = parse(text, Some(Format::Csv)).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].title, "A");
        assert_eq!(entries[1].title, "B");
    }

    #[test]
    fn csv_over_the_entry_limit_is_refused() {
        let mut text = String::from("Title,Username\n");
        for i in 0..(MAX_IMPORT_ENTRIES + 1) {
            text.push_str(&format!("entry-{i},user-{i}\n"));
        }
        let err = parse(&text, Some(Format::Csv)).unwrap_err();
        assert!(err.to_string().contains("条目太多"), "{err}");
    }

    /// 上限必须落在**展开记录**这一步,而不是只拦最终的 Entry ——
    /// 否则一个几百万行的 CSV 已经把内存吃掉了,再报错也来不及。
    #[test]
    fn csv_record_expansion_stops_at_the_row_limit() {
        let mut text = String::from("Title,Username\n");
        for i in 0..(MAX_IMPORT_ENTRIES + 5) {
            text.push_str(&format!("e{i},u{i}\n"));
        }
        let err = parse_csv_records(&text).unwrap_err();
        assert!(err.to_string().contains("条目太多"), "{err}");
    }

    #[test]
    fn json_over_the_entry_limit_is_refused() {
        let items: Vec<String> = (0..MAX_IMPORT_ENTRIES + 1)
            .map(|i| format!(r#"{{"title":"e{i}","username":"u{i}"}}"#))
            .collect();
        let text = format!("[{}]", items.join(","));
        let err = parse(&text, Some(Format::Json)).unwrap_err();
        assert!(err.to_string().contains("条目太多"), "{err}");
    }

    /// 刚好卡在上限的文件要能正常导入 —— 上限是「拒绝超过」,不是「拒绝达到」。
    #[test]
    fn exactly_at_the_entry_limit_is_accepted() {
        let mut text = String::from("Title,Username\n");
        for i in 0..MAX_IMPORT_ENTRIES {
            text.push_str(&format!("e{i},u{i}\n"));
        }
        let entries = parse(&text, Some(Format::Csv)).unwrap();
        assert_eq!(entries.len(), MAX_IMPORT_ENTRIES);
    }

    #[test]
    fn split_tags_dedupes_and_trims() {
        assert_eq!(
            split_tags("a; b |a;;"),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
