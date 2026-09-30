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
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
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

    pub fn touch(&mut self) {
        self.updated = now_secs();
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
    /// 窗口置顶。
    pub always_on_top: bool,
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
            always_on_top: false,
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

        // 旧版本 JSON 没有 column_widths 字段,能正常反序列化
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

        // 默认状态下空 column_widths 不会被序列化出来
        let serialized = serde_json::to_string(&s).expect("序列化失败");
        assert!(!serialized.contains("column_widths"));

        // 自定义列宽时正常序列化与反序列化
        let mut custom = s;
        custom.column_widths = vec![147, 113, 170, 80, 160, 150];
        let serialized_custom = serde_json::to_string(&custom).expect("序列化失败");
        assert!(serialized_custom.contains("column_widths"));
        let roundtrip: Settings = serde_json::from_str(&serialized_custom).expect("反序列化失败");
        assert_eq!(roundtrip.column_widths, vec![147, 113, 170, 80, 160, 150]);
    }
}
