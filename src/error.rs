use std::fmt;

#[derive(Debug)]
pub enum VaultError {
    /// 文件读写失败。
    Io(std::io::Error),
    /// 文件结构损坏或不是密码本。
    Format(String),
    /// 加解密失败(除「密码错误」外的技术性失败)。
    Crypto(String),
    /// 主密码或恢复码不正确。
    WrongSecret(&'static str),
    /// 处于锁定状态。
    Locked,
    /// 找不到目标条目。
    NotFound,
    /// 输入不合法。
    Invalid(String),
}

impl fmt::Display for VaultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VaultError::Io(e) => write!(f, "文件操作失败:{e}"),
            VaultError::Format(m) => write!(f, "{m}"),
            VaultError::Crypto(m) => write!(f, "{m}"),
            VaultError::WrongSecret(m) => write!(f, "{m}"),
            VaultError::Locked => write!(f, "密码本处于锁定状态。"),
            VaultError::NotFound => write!(f, "找不到该条目。"),
            VaultError::Invalid(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<std::io::Error> for VaultError {
    fn from(value: std::io::Error) -> Self {
        VaultError::Io(value)
    }
}

impl From<serde_json::Error> for VaultError {
    fn from(value: serde_json::Error) -> Self {
        VaultError::Format(format!("内容解析失败:{value}"))
    }
}

pub type Result<T> = std::result::Result<T, VaultError>;
