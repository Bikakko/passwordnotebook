//! 密码本 —— 可移植核心库。
//!
//! 本 crate 的所有模块都不依赖 Windows API,因此可以在任意平台上跑单元测试;
//! 真正的界面实现位于 `win` 模块(仅在 Windows 上编译)。

pub mod crypto;
pub mod error;
pub mod export_import;
pub mod generator;
pub mod header;
pub mod model;
pub mod paths;
pub mod recovery;
pub mod search;
pub mod strength;
pub mod url;
pub mod vault;
pub mod vaultfile;

#[cfg(windows)]
pub mod win;
