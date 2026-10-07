//! 数据模型。序列化为 JSON 后作为载荷被加密。

use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::crypto;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub username: String,
    /// 用 `Zeroizing`:条目在回收/丢弃时这块内存会被抹掉,而不是留在堆上。
    #[serde(default)]
    pub password: Zeroizing<String>,
    #[serde(default)]
    pub url: String,
    /// API 服务的密钥。与密码同为敏感字段,同样用 `Zeroizing` 管理。
    #[serde(default)]
    pub api_key: Zeroizing<String>,
    /// API 服务的端点(base url)。普通文本,与 `url` 同待遇。
    #[serde(default)]
    pub api_endpoint: String,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    /// 收藏:常用条目在列表里置顶显示。
    ///
    /// `serde(default)`:0.1.x 写出的 JSON 没有这个字段,读旧库时必须当「未收藏」。
    #[serde(default)]
    pub favorite: bool,
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub updated: i64,
    #[serde(default)]
    pub deleted: Option<i64>,
}

impl Entry {
    pub fn new_id() -> Result<String, crate::error::VaultError> {
        Ok(hex(&crypto::random_array::<16>()?))
    }

    pub fn is_deleted(&self) -> bool {
        self.deleted.is_some()
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

pub fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub version: u32,
    pub created: i64,
    /// 设置也加密存在库里 —— 磁盘上只有 data.pkk 一个数据文件。
    #[serde(default)]
    pub settings: Settings,
    /// 已创建的分类(与条目分开维护,先建后用)。
    #[serde(default)]
    pub categories: Vec<String>,
    /// 已创建的标签(同上)。
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Default for Document {
    fn default() -> Self {
        Self {
            version: 1,
            created: now_secs(),
            settings: Settings::default(),
            categories: Vec::new(),
            tags: Vec::new(),
            entries: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 允许本机免密解锁。
    pub quick_unlock_enabled: bool,
    /// 免密解锁前要求 Windows Hello 验证。
    pub require_windows_hello: bool,
    /// 自定义空闲锁定分钟数:0 跟随系统屏保,-1 不自动锁定。
    pub idle_lock_minutes: i32,
    /// 复制密码后自动清空剪贴板的秒数;0 表示不清空。
    pub clipboard_clear_seconds: u32,
    /// 回收站保留天数。
    pub bin_retention_days: i64,
    /// 「只看收藏」筛选开关的最后状态(跟着库走)。
    pub favorites_only: bool,
    /// 自定义列表各列宽度(逻辑像素, 96 DPI 基准):[标题, 用户名, 网址, 分类, 标签, 更新时间]。
    /// 若为空表示未调整过,使用默认长度与比例。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub column_widths: Vec<i32>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            quick_unlock_enabled: true,
            require_windows_hello: false,
            idle_lock_minutes: 0,
            clipboard_clear_seconds: 20,
            bin_retention_days: 30,
            favorites_only: false,
            column_widths: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_default_and_backward_compatibility() {
        let default_settings = Settings::default();
        assert!(default_settings.column_widths.is_empty());
        assert!(!default_settings.favorites_only);

        // 旧版本 JSON 没有 column_widths / favorites_only 字段,能正常反序列化。
        // 这里还刻意带着已移除的 always_on_top 字段:旧文件里有它时也必须能读(未知字段被忽略)。
        let old_json = r#"{
            "quick_unlock_enabled": true,
            "require_windows_hello": false,
            "idle_lock_minutes": 0,
            "clipboard_clear_seconds": 20,
            "bin_retention_days": 30,
            "always_on_top": false
        }"#;
        let s: Settings = serde_json::from_str(old_json).expect("反序列化旧版设置失败");
        assert!(s.column_widths.is_empty());
        assert!(!s.favorites_only, "旧库没有该字段时应当作未勾选");

        // 默认状态下空 column_widths 不会被序列化出来
        let serialized = serde_json::to_string(&s).expect("序列化失败");
        assert!(!serialized.contains("column_widths"));
        assert!(!serialized.contains("always_on_top"), "已移除的字段不应再被写进文件");

        // 自定义列宽与「只看收藏」时正常序列化与反序列化
        let mut custom = s;
        custom.column_widths = vec![147, 113, 170, 80, 160, 150];
        custom.favorites_only = true;
        let serialized_custom = serde_json::to_string(&custom).expect("序列化失败");
        assert!(serialized_custom.contains("column_widths"));
        let roundtrip: Settings = serde_json::from_str(&serialized_custom).expect("反序列化失败");
        assert_eq!(roundtrip.column_widths, vec![147, 113, 170, 80, 160, 150]);
        assert!(roundtrip.favorites_only);
    }

    /// 0.1.x 写出的 JSON 没有 favorite 字段;新版必须能读,并当成「未收藏」。
    #[test]
    fn entry_without_favorite_field_defaults_to_false() {
        let old_json = r#"{
            "id": "abc",
            "title": "GitHub",
            "username": "alice",
            "password": "pw",
            "url": "https://github.com",
            "notes": "",
            "category": "工作",
            "tags": ["代码"],
            "created": 1,
            "updated": 2,
            "deleted": null
        }"#;
        let entry: Entry = serde_json::from_str(old_json).expect("旧版条目应能反序列化");
        assert!(!entry.favorite, "缺少 favorite 时应默认未收藏");
        assert_eq!(entry.title, "GitHub");
        assert_eq!(entry.password.as_str(), "pw");

        // 写回时带上新字段,方便以后再升级。
        let json = serde_json::to_string(&entry).expect("序列化失败");
        assert!(json.contains("\"favorite\":false"));

        // 新字段加上后仍可往返。
        let mut favored = entry;
        favored.favorite = true;
        let json = serde_json::to_string(&favored).unwrap();
        let back: Entry = serde_json::from_str(&json).unwrap();
        assert!(back.favorite);
    }

    /// 0.2.x 写出的 JSON 没有 api_key / api_endpoint 字段;新版必须能读,并当成空。
    #[test]
    fn entry_without_api_fields_defaults_to_empty() {
        let old_json = r#"{
            "id": "abc",
            "title": "服务",
            "username": "alice",
            "password": "pw",
            "url": "https://example.com",
            "notes": "",
            "category": "",
            "tags": [],
            "favorite": false,
            "created": 1,
            "updated": 2,
            "deleted": null
        }"#;
        let entry: Entry = serde_json::from_str(old_json).expect("旧版条目应能反序列化");
        assert!(entry.api_key.is_empty(), "缺少 api_key 时应默认为空");
        assert!(entry.api_endpoint.is_empty(), "缺少 api_endpoint 时应默认为空");

        // 写回时带上新字段,供 0.3.0 起的版本再读。
        let json = serde_json::to_string(&entry).expect("序列化失败");
        assert!(json.contains("\"api_key\":\"\""));

        let mut with_api = entry;
        with_api.api_key = Zeroizing::new("sk-123".into());
        with_api.api_endpoint = "https://api.example.com/v1".into();
        let json = serde_json::to_string(&with_api).unwrap();
        let back: Entry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.api_key.as_str(), "sk-123");
        assert_eq!(back.api_endpoint, "https://api.example.com/v1");
    }
}
