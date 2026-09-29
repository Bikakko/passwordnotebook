//! 本机免密解锁缓存:用 Windows DPAPI(CurrentUser)包裹数据密钥后落盘。
//!
//! 缓存与保险库的 `vault_id + key_generation` 绑定。失效有两条独立途径:
//!
//! 1. **改本程序的登录密码** —— 重包主密码槽时会把 `key_generation` 加一
//!    (见 `vault.rs` 的 `change_master_password` / `reset_master_password`),
//!    缓存里存的代数于是对不上;改密后界面层还会再 [`clear`] 一次。
//! 2. **缓存文件被删除** —— 检测到锁屏/屏保时由界面层 [`clear`],
//!    或在设置里关闭免密解锁。
//!
//! 注意第 1 条指的是**本程序的登录密码,不是 Windows 登录密码**:
//!
//! - DPAPI 只绑定**本机这个 Windows 账户**。用户改自己的 Windows 密码时,
//!   Windows 会用新密码重新包裹该账户的主密钥,已生成的 DPAPI 数据照旧可解 ——
//!   **想靠改 Windows 密码来让这个缓存失效并不可靠**。(只有管理员用「重置密码」
//!   而非「更改密码」时才会毁掉该账户的主密钥,那种情况下缓存读不出来,
//!   会退回要求输入登录密码,属于优雅降级。)
//! - 反过来,同一 Windows 账户下的**任何进程**都能解密它。所以这个缓存的强度
//!   大致等于「Windows 账户本身」,而不是强加密。要更严就勾选
//!   「免密解锁时要求 Windows Hello 验证」,或关闭免密解锁。

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::Foundation::LocalFree;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
};
use zeroize::Zeroizing;

const MAGIC: [u8; 4] = *b"PNBQ";
const FLAG_REQUIRE_HELLO: u8 = 0x01;
/// 发生锁屏/手动锁定后置位:下次启动需要验证,而不是直接免密进入。
const FLAG_LOCKED: u8 = 0x02;

fn entropy() -> Vec<u8> {
    b"PasswordNotebook.QuickUnlock.v1".to_vec()
}

pub fn cache_path() -> Option<PathBuf> {
    let base = std::env::var("LOCALAPPDATA").ok()?;
    Some(PathBuf::from(base).join("PasswordNotebook").join("quickunlock.dat"))
}

pub fn exists() -> bool {
    cache_path().is_some_and(|p| p.exists())
}

/// 写入缓存(失败时静默忽略,不影响正常使用)。
pub fn store(vault_id: [u8; 16], key_generation: u32, dek: &[u8], require_hello: bool) {
    let Some(path) = cache_path() else { return };
    let Ok(protected) = protect(dek) else { return };

    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }

    let mut out = Vec::with_capacity(25 + protected.len());
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(&vault_id);
    out.extend_from_slice(&key_generation.to_le_bytes());
    out.push(if require_hello { FLAG_REQUIRE_HELLO } else { 0 });
    out.extend_from_slice(&(protected.len() as u32).to_le_bytes());
    out.extend_from_slice(&protected);

    let _ = std::fs::write(path, out);
}

/// 读取缓存;vault_id / key_generation 不匹配或解密失败时返回 None。
pub fn load(vault_id: [u8; 16], key_generation: u32) -> Option<(Zeroizing<Vec<u8>>, bool, bool)> {
    let path = cache_path()?;
    let bytes = std::fs::read(path).ok()?;

    if bytes.len() < 25 || bytes[0..4] != MAGIC {
        return None;
    }
    if bytes[4..20] != vault_id {
        return None;
    }
    if u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]) != key_generation {
        return None;
    }

    let require_hello = bytes[24] & FLAG_REQUIRE_HELLO != 0;
    let locked = bytes[24] & FLAG_LOCKED != 0;
    let len = u32::from_le_bytes([bytes[25], bytes[26], bytes[27], bytes[28]]) as usize;
    if bytes.len() < 29 + len {
        return None;
    }

    let dek = unprotect(&bytes[29..29 + len]).ok()?;
    if dek.len() != 32 {
        return None;
    }
    Some((dek, require_hello, locked))
}

/// 锁屏 / 手动锁定时调用。
///
/// - 缓存要求 Windows Hello:保留缓存但打上「已锁定」标记 → 下次启动用指纹/人脸验证
/// - 否则:直接删除缓存 → 下次必须输入登录密码
pub fn invalidate_on_lock() {
    let Some(path) = cache_path() else { return };
    let Ok(bytes) = std::fs::read(&path) else { return };

    if bytes.len() < 25 || bytes[24] & FLAG_REQUIRE_HELLO == 0 {
        let _ = std::fs::remove_file(&path);
        return;
    }

    let mut marked = bytes;
    marked[24] |= FLAG_LOCKED;
    let _ = std::fs::write(&path, marked);
}

pub fn clear() {
    if let Some(path) = cache_path() {
        let _ = std::fs::remove_file(path);
    }
}

fn blob(data: &[u8]) -> CRYPT_INTEGER_BLOB {
    CRYPT_INTEGER_BLOB {
        cbData: data.len() as u32,
        pbData: data.as_ptr() as *mut u8,
    }
}

fn protect(data: &[u8]) -> Result<Vec<u8>, String> {
    let secret = entropy();
    let input = blob(data);
    let ent = blob(&secret);
    let mut output = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptProtectData(
            &input,
            PCWSTR::null(),
            Some(&ent),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| e.to_string())?;

        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(output.pbData as *mut _)));
        Ok(result)
    }
}

fn unprotect(data: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let secret = entropy();
    let input = blob(data);
    let ent = blob(&secret);
    let mut output = CRYPT_INTEGER_BLOB::default();

    unsafe {
        CryptUnprotectData(
            &input,
            None,
            Some(&ent),
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| e.to_string())?;

        let result = Zeroizing::new(
            std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec(),
        );
        // output.pbData 是系统分配的、装着**明文 DEK** 的缓冲。
        // LocalFree 只是把内存还给堆,不会清零 —— 不清就会留下可被翻出的副本。
        std::ptr::write_bytes(output.pbData, 0, output.cbData as usize);
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(output.pbData as *mut _)));
        Ok(result)
    }
}
