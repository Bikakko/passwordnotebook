//! 导入 / 导出:CSV 与 JSON。
//!
//! 两条通道的分工是刻意分开的:
//!
//! - **CSV 面向互操作**:固定 7 列(`Title,Username,Password,Url,Category,Tags,Notes`),
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
pub const CSV_HEADER: [&str; 7] = ["Title", "Username", "Password", "Url", "Category", "Tags", "Notes"];

/// 标签在 CSV 单元格内的分隔符(`|` 也接受)。
pub const TAG_SEPARATOR: char = ';';

/// 导出格式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// 逗号分隔,UTF-8 带 BOM,Excel 友好。
    Csv,
    /// 自描述的 JSON 信封,备份友好。
    Json,
}

impl Format {
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
            Format::Csv => "CSV(Excel 可打开)",
            Format::Json => "JSON(完整备份)",
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
    /// 是否把密码写成明文。关闭时密码列/字段整体省略,而不是留空 ——
    /// 留空会在「覆盖导入」时被误读成「把密码清空」。
    pub include_passwords: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            include_passwords: true,
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
    /// 已有同名条目则用导入项覆盖它(密码为空时不覆盖原密码)。
    Overwrite,
}

impl DuplicateStrategy {
    pub fn label(self) -> &'static str {
        match self {
            DuplicateStrategy::Append => "全部新增(不比对)",
            DuplicateStrategy::Skip => "跳过重复项",
            DuplicateStrategy::Overwrite => "重复项用文件内容覆盖",
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

    pub fn total(&self) -> usize {
        self.added + self.updated + self.skipped
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
            "没有可导入的记录。".to_string()
        } else {
            parts.join(",")
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
pub fn import_from_path(path: &Path) -> Result<Vec<Entry>> {
    // 文件内容是明文(可能含密码),读完立刻纳入 Zeroizing 管理。
    let bytes = Zeroizing::new(std::fs::read(path)?);
    let text = decode_utf8(bytes.as_slice())?;
    parse(&text, None)
}

/// 按内容解析导入文本;`hint` 为 `None` 时自动判断 JSON / CSV。
pub fn parse(text: &str, hint: Option<Format>) -> Result<Vec<Entry>> {
    let text = text.trim_start_matches('\u{feff}');
    if text.trim().is_empty() {
        return Err(VaultError::Invalid("文件里没有任何内容。".into()));
    }
    match hint.unwrap_or_else(|| detect_format(text)) {
        Format::Json => parse_json(text),
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
            "文件不是 UTF-8 编码(第 {} 字节处出错)。请在 Excel 里用「CSV UTF-8(逗号分隔)」重新保存,或先转成 UTF-8 再导入。",
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

fn export_csv(document: &Document, options: ExportOptions) -> String {
    // 带 BOM:否则 Excel 会把中文按本地代码页解读成乱码。
    let mut out = String::from("\u{feff}");
    let header: Vec<&str> = CSV_HEADER
        .iter()
        .copied()
        .filter(|c| options.include_passwords || *c != "Password")
        .collect();
    out.push_str(&header.join(","));
    out.push_str("\r\n");

    for entry in active_entries(document) {
        let mut cells: Vec<String> = vec![
            entry.title.clone(),
            entry.username.clone(),
        ];
        if options.include_passwords {
            cells.push(entry.password.as_str().to_string());
        }
        cells.push(entry.url.clone());
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
    category: &'a str,
    tags: &'a [String],
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
            password: options.include_passwords.then(|| e.password.as_str()),
            url: &e.url,
            category: &e.category,
            tags: &e.tags,
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
    Category,
    Tags,
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
const CATEGORY_ALIASES: &[&str] = &["category", "folder", "group", "grouping", "分类", "分组", "文件夹", "目录"];
const TAGS_ALIASES: &[&str] = &["tags", "tag", "labels", "label", "标签"];
const NOTES_ALIASES: &[&str] = &["notes", "note", "comment", "comments", "extra", "memo", "备注", "说明", "注释"];
const ID_ALIASES: &[&str] = &["id", "uuid", "标识"];
const CREATED_ALIASES: &[&str] = &["created", "createdat", "creationtime", "创建时间", "创建于"];
const UPDATED_ALIASES: &[&str] = &[
    "updated", "updatedat", "modified", "modifiedat", "lastmodified", "更新时间", "修改时间",
];

/// 表头/字段名 → 规范字段。未知字段返回 `None`(直接忽略,例如 Bitwarden 的 `favorite`)。
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
        (CATEGORY_ALIASES, Field::Category),
        (TAGS_ALIASES, Field::Tags),
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
    let records = parse_csv_records(text);
    let header_index = records
        .iter()
        .position(|r| r.iter().any(|c| !c.trim().is_empty()))
        .ok_or_else(|| VaultError::Invalid("文件里没有任何内容。".into()))?;

    let header: Vec<Option<Field>> = records[header_index].iter().map(|h| field_from_key(h)).collect();

    if !header
        .iter()
        .any(|f| matches!(f, Some(Field::Title) | Some(Field::Username) | Some(Field::Password)))
    {
        return Err(VaultError::Invalid(
            "识别不出表头。第一行需要包含标题/名称、用户名或密码之类的列名。".into(),
        ));
    }

    let mut out = Vec::new();
    for record in &records[header_index + 1..] {
        if record.iter().all(|c| c.trim().is_empty()) {
            continue;
        }
        let entry = entry_from_row(&header, record);
        if is_blank_entry(&entry) {
            continue;
        }
        out.push(entry);
    }

    if out.is_empty() {
        return Err(VaultError::Invalid("表头之后没有任何数据行。".into()));
    }
    Ok(out)
}

fn entry_from_row(header: &[Option<Field>], record: &[String]) -> Entry {
    let mut entry = Entry::default();
    let mut tags: Vec<String> = Vec::new();

    for (index, field) in header.iter().enumerate() {
        let Some(field) = field else { continue };
        let raw = record.get(index).map(String::as_str).unwrap_or("");
        let value = unguard_formula(raw);
        match field {
            Field::Id => entry.id = value.trim().to_string(),
            Field::Title => entry.title = value,
            Field::Username => entry.username = value,
            Field::Password => entry.password = Zeroizing::new(value),
            Field::Url => entry.url = value,
            Field::Category => entry.category = value,
            Field::Tags => tags.extend(split_tags(&value)),
            Field::Notes => entry.notes = value,
            Field::Created => entry.created = parse_timestamp(&value),
            Field::Updated => entry.updated = parse_timestamp(&value),
        }
    }

    entry.tags = tags;
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
pub fn is_blank_entry(entry: &Entry) -> bool {
    entry.title.trim().is_empty()
        && entry.username.trim().is_empty()
        && entry.password.is_empty()
        && entry.url.trim().is_empty()
        && entry.notes.trim().is_empty()
}

/// RFC 4180 解析器,额外容忍 BOM、CRLF/CR/LF 与字段内换行。
fn parse_csv_records(text: &str) -> Vec<Vec<String>> {
    let mut records: Vec<Vec<String>> = Vec::new();
    let mut record: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.trim_start_matches('\u{feff}').chars().peekable();

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
                records.push(std::mem::take(&mut record));
            }
            _ => field.push(c),
        }
    }

    if !field.is_empty() || !record.is_empty() {
        record.push(field);
        records.push(record);
    }
    records
}

fn parse_json(text: &str) -> Result<Vec<Entry>> {
    let value: Value = serde_json::from_str(text)?;

    let items: &Vec<Value> = match &value {
        Value::Array(items) => items,
        Value::Object(map) => match map.get("entries") {
            Some(Value::Array(items)) => items,
            _ => {
                return Err(VaultError::Invalid(
                    "JSON 里找不到 entries 数组(可以导出为 CSV 再试)。".into(),
                ));
            }
        },
        _ => {
            return Err(VaultError::Invalid(
                "JSON 顶层既不是对象也不是数组。".into(),
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
        out.push(entry);
    }

    if out.is_empty() {
        return Err(VaultError::Invalid("JSON 里没有任何可导入的条目。".into()));
    }
    Ok(out)
}

fn entry_from_object(map: &Map<String, Value>) -> Entry {
    let mut entry = Entry::default();
    let mut tags: Vec<String> = Vec::new();

    for (key, value) in map {
        let Some(field) = field_from_key(key) else { continue };
        match field {
            Field::Id => entry.id = as_string(value),
            Field::Title => entry.title = as_string(value),
            Field::Username => entry.username = as_string(value),
            Field::Password => entry.password = Zeroizing::new(as_string(value)),
            Field::Url => entry.url = as_string(value),
            Field::Category => entry.category = as_string(value),
            Field::Tags => match value {
                Value::Array(items) => {
                    for item in items {
                        for tag in split_tags(&as_string(item)) {
                            if !tags.iter().any(|t| t.eq_ignore_ascii_case(&tag)) {
                                tags.push(tag);
                            }
                        }
                    }
                }
                other => tags.extend(split_tags(&as_string(other))),
            },
            Field::Notes => entry.notes = as_string(value),
            Field::Created => entry.created = parse_timestamp(&as_string(value)),
            Field::Updated => entry.updated = parse_timestamp(&as_string(value)),
        }
    }

    entry.tags = tags;
    entry
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
pub fn dedupe_key(entry: &Entry) -> Option<String> {
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

    fn sample_document() -> Document {
        let mut document = Document::default();
        document.categories = vec!["工作".into()];
        document.tags = vec!["重要".into()];
        document.entries = vec![
            Entry {
                title: "带,逗号".into(),
                username: "a\"b".into(),
                password: Zeroizing::new("p,w\"d\n换行".into()),
                url: "https://example.com".into(),
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
        ];
        document
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
        assert_eq!(parsed[1].tags, vec!["重要".to_string(), "内部".to_string()]);
        assert_eq!(parsed[1].notes, "第一行\n第二行");
    }

    #[test]
    fn csv_has_bom_and_crlf() {
        let text = export_csv(&sample_document(), ExportOptions::default());
        assert!(text.starts_with('\u{feff}'));
        assert!(text.contains("\r\n"));
        assert!(text.starts_with("\u{feff}Title,Username,Password,Url,Category,Tags,Notes"));
    }

    #[test]
    fn csv_omits_password_column_when_disabled() {
        let document = sample_document();
        let text = export_csv(
            &document,
            ExportOptions {
                include_passwords: false,
            },
        );
        assert!(!text.contains("secret"));
        assert!(text.starts_with("\u{feff}Title,Username,Url,Category,Tags,Notes"));
    }

    #[test]
    fn formula_prefixes_are_guarded_and_listed() {
        let mut document = Document::default();
        document.entries = vec![Entry {
            title: "=cmd|'/C calc'!A1".into(),
            username: "@SUM(1)".into(),
            password: Zeroizing::new("-2+3".into()),
            ..Default::default()
        }];

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
        let mut expected: Vec<(String, String, String, String, String, String)> = Vec::new();
        for index in 0..300 {
            let title = random_field();
            let password = random_field();
            let url = random_field();
            let category = random_field();
            let notes = random_field();
            let tags = random_field();
            expected.push((
                title.clone(),
                password.clone(),
                url.clone(),
                category.clone(),
                notes.clone(),
                tags.clone(),
            ));
            document.entries.push(Entry {
                // 用户名固定非空:否则整条全空的行会在解析时被当作空行丢掉。
                title,
                username: format!("user-{index}"),
                password: Zeroizing::new(password),
                url,
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
    fn json_accepts_bare_array_and_aliases() {
        let text = r#"[{"name":"标题","login":"用户","password":"p","tags":"a|b"}]"#;
        let parsed = parse_json(text).unwrap();
        assert_eq!(parsed[0].title, "标题");
        assert_eq!(parsed[0].username, "用户");
        assert_eq!(parsed[0].tags, vec!["a".to_string(), "b".to_string()]);
    }

    #[test]
    fn json_import_ignores_unknown_keys() {
        let text = r#"{"format":"password-notebook","version":1,"entries":[{"title":"T","favorite":true,"totp":"x"}]}"#;
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
        let mut a = Entry::default();
        a.title = " GitHub ".into();
        a.username = "Me".into();
        let mut b = Entry::default();
        b.title = "github".into();
        b.username = "me".into();
        assert_eq!(dedupe_key(&a), dedupe_key(&b));

        let mut c = Entry::default();
        c.url = "https://Only-Url.example".into();
        let mut d = Entry::default();
        d.url = "https://only-url.example".into();
        assert_eq!(dedupe_key(&c), dedupe_key(&d));

        assert_eq!(dedupe_key(&Entry::default()), None);
    }

    #[test]
    fn csv_parser_handles_embedded_newlines_and_quotes() {
        let text = "Title,Notes\n\"多行\",\"第一行\n第二行\"\n\"引号\"\"内部\",x\n";
        let records = parse_csv_records(text);
        assert_eq!(records.len(), 3);
        assert_eq!(records[1][1], "第一行\n第二行");
        assert_eq!(records[2][0], "引号\"内部");
    }

    #[test]
    fn split_tags_dedupes_and_trims() {
        assert_eq!(
            split_tags("a; b |a;;"),
            vec!["a".to_string(), "b".to_string()]
        );
    }
}
