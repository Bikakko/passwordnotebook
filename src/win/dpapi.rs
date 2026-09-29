//! 本机免密解锁缓存:用 Windows DPAPI(CurrentUser)包裹数据密钥后落盘。
//!
//! 缓存与保险库的 vault_id + key_generation 绑定,改登录密码后自动失效;
//! 检测到锁屏/屏保时由界面层调用 [`clear`] 删除。

use std::path::PathBuf;

use windows::core::PCWSTR;
use windows::Win32::Foundation::LocalFree;
use windows::Win32::Security::Cryptography::{
    CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN,
};

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
pub fn load(vault_id: [u8; 16], key_generation: u32) -> Option<(Vec<u8>, bool, bool)> {
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
    let mut secret = entropy();
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
        secret.fill(0);
        Ok(result)
    }
}

fn unprotect(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut secret = entropy();
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

        let result = std::slice::from_raw_parts(output.pbData, output.cbData as usize).to_vec();
        let _ = LocalFree(Some(windows::Win32::Foundation::HLOCAL(output.pbData as *mut _)));
        secret.fill(0);
        Ok(result)
    }
}
