//! 数据模型。序列化为 JSON 后作为载荷被加密。

use serde::{Deserialize, Serialize};

use crate::crypto;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
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
        }
    }
}
