//! 保险库服务:解锁、持久化、以及所有条目与分类的读写。
//!
//! 数据密钥(DEK)只存在于内存中,`lock()` 或进程退出后即不可恢复。

use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::crypto::{self, KEY_LEN};
use crate::error::{Result, VaultError};
use crate::header::VaultHeader;
use crate::model::{now_secs, Document, Entry};
use crate::recovery;
use crate::vaultfile::VaultFile;

pub struct VaultService {
    file: Option<VaultFile>,
    dek: Option<Zeroizing<Vec<u8>>>,
    document: Option<Document>,
    path: Option<PathBuf>,
    unlocked_with_recovery: bool,
}

impl Default for VaultService {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultService {
    pub fn new() -> Self {
        Self {
            file: None,
            dek: None,
            document: None,
            path: None,
            unlocked_with_recovery: false,
        }
    }

    // ---------- 状态 ----------

    pub fn is_unlocked(&self) -> bool {
        self.dek.is_some() && self.document.is_some() && self.file.is_some()
    }

    pub fn document(&self) -> Option<&Document> {
        self.document.as_ref()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub fn unlocked_with_recovery(&self) -> bool {
        self.unlocked_with_recovery
    }

    pub fn lock(&mut self) {
        self.dek = None;
        self.document = None;
        self.file = None;
        self.unlocked_with_recovery = false;
    }

    /// 读取文件头(不需要解锁),用于判断本机免密缓存是否可用。
    pub fn peek_header(path: &Path) -> Result<VaultHeader> {
        Ok(VaultFile::read(path)?.header)
    }

    /// 只校验主密码是否正确,不改变当前解锁状态。
    pub fn verify_master_password(path: &Path, master_password: &str) -> bool {
        match VaultFile::read(path) {
            Ok(file) => {
                let h = &file.header;
                unwrap_with_secret(
                    master_password.as_bytes(),
                    &h.password_salt,
                    &h.password_nonce,
                    &file.password_wrapped,
                    &h.password_slot_aad(),
                    h.m_cost_kib,
                    h.t_cost,
                    h.p_cost,
                )
                .is_ok()
            }
            Err(_) => false,
        }
    }

    // ---------- 创建与解锁 ----------

    /// 创建新密码本,返回一次性恢复码。
    pub fn create_new(path: &Path, master_password: &str) -> Result<String> {
        Self::create_new_with_params(
            path,
            master_password,
            crypto::DEFAULT_M_COST_KIB,
            crypto::DEFAULT_T_COST,
            crypto::DEFAULT_P_COST,
        )
    }

    /// 指定 KDF 参数创建密码本(仅测试会用到非默认参数)。
    pub fn create_new_with_params(
        path: &Path,
        master_password: &str,
        m_cost_kib: u32,
        t_cost: u32,
        p_cost: u32,
    ) -> Result<String> {
        let header = VaultHeader::generate_with(m_cost_kib, t_cost, p_cost)?;
        let dek = Zeroizing::new(crypto::random(KEY_LEN)?);
        let recovery_code = recovery::generate()?;
        let normalized = recovery::normalize(&recovery_code)
            .ok_or_else(|| VaultError::Crypto("恢复码生成异常。".into()))?;

        let document = Document::default();
        // 整份库的明文(JSON)在堆上只活这一小会儿,用完立刻抹掉。
        let plain = Zeroizing::new(serde_json::to_vec(&document)?);
        let payload = crypto::seal(&dek, &header.payload_nonce, &plain, &header.payload_aad())?;

        let password_wrapped = wrap_with_secret(
            master_password.as_bytes(),
            &header.password_salt,
            &header.password_nonce,
            &dek,
            &header.password_slot_aad(),
            header.m_cost_kib,
            header.t_cost,
            header.p_cost,
        )?;

        let recovery_wrapped = wrap_with_secret(
            normalized.as_bytes(),
            &header.recovery_salt,
            &header.recovery_nonce,
            &dek,
            &header.recovery_slot_aad(),
            header.m_cost_kib,
            header.t_cost,
            header.p_cost,
        )?;

        VaultFile {
            header,
            password_wrapped,
            recovery_wrapped,
            payload,
        }
        .write_atomic(path)?;

        Ok(recovery_code)
    }

    pub fn open(&mut self, path: &Path, master_password: &str) -> Result<()> {
        let file = VaultFile::read(path)?;
        let h = &file.header;
        let dek = unwrap_with_secret(
            master_password.as_bytes(),
            &h.password_salt,
            &h.password_nonce,
            &file.password_wrapped,
            &h.password_slot_aad(),
            h.m_cost_kib,
            h.t_cost,
            h.p_cost,
        )?;
        self.adopt(path, file, dek, false)
    }

    pub fn open_with_recovery_code(&mut self, path: &Path, code: &str) -> Result<()> {
        let normalized = recovery::normalize(code)
            .ok_or(VaultError::WrongSecret("恢复码格式不正确,请检查后重试。"))?;

        let file = VaultFile::read(path)?;
        let h = &file.header;
        let dek = unwrap_with_secret(
            normalized.as_bytes(),
            &h.recovery_salt,
            &h.recovery_nonce,
            &file.recovery_wrapped,
            &h.recovery_slot_aad(),
            h.m_cost_kib,
            h.t_cost,
            h.p_cost,
        )?;
        self.adopt(path, file, dek, true)
    }

    /// 用本机快速解锁缓存中的数据密钥直接打开(已由 DPAPI 解出)。
    pub fn open_with_cached_key(&mut self, path: &Path, dek: Zeroizing<Vec<u8>>) -> Result<()> {
        let file = VaultFile::read(path)?;
        // 解密出来的是整份库的明文,用完立刻抹掉。
        let plain = Zeroizing::new(
            crypto::open(
                &dek,
                &file.header.payload_nonce,
                &file.payload,
                &file.header.payload_aad(),
            )
            .map_err(|_| VaultError::WrongSecret("本机免密缓存已失效,请输入主密码。"))?,
        );

        let document: Document = serde_json::from_slice(&plain)?;
        self.file = Some(file);
        self.dek = Some(dek);
        self.document = Some(document);
        self.path = Some(path.to_path_buf());
        self.unlocked_with_recovery = false;
        Ok(())
    }

    fn adopt(&mut self, path: &Path, file: VaultFile, dek: Zeroizing<Vec<u8>>, via_recovery: bool) -> Result<()> {
        // 解密出来的是整份库的明文,用完立刻抹掉。
        let plain = Zeroizing::new(crypto::open(
            &dek,
            &file.header.payload_nonce,
            &file.payload,
            &file.header.payload_aad(),
        )?);
        let document: Document = serde_json::from_slice(&plain)?;

        self.file = Some(file);
        self.dek = Some(dek);
        self.document = Some(document);
        self.path = Some(path.to_path_buf());
        self.unlocked_with_recovery = via_recovery;
        Ok(())
    }

    // ---------- 保存 ----------

    pub fn save(&mut self) -> Result<()> {
        let path = self.path.clone().ok_or(VaultError::Locked)?;

        let document = self.document.as_ref().ok_or(VaultError::Locked)?;
        let dek = self.dek.as_ref().ok_or(VaultError::Locked)?;
        let file = self.file.as_mut().ok_or(VaultError::Locked)?;

        file.header.payload_nonce = crypto::random_array()?;
        // 待加密的明文同样只活这一小会儿。
        let plain = Zeroizing::new(serde_json::to_vec(document)?);
        file.payload = crypto::seal(
            dek,
            &file.header.payload_nonce,
            &plain,
            &file.header.payload_aad(),
        )?;

        file.write_atomic(&path)
    }

    /// 取出给本机免密缓存用的材料:(vault_id, key_generation, DEK 副本)。
    pub fn quick_unlock_material(&self) -> Option<([u8; 16], u32, Zeroizing<Vec<u8>>)> {
        let file = self.file.as_ref()?;
        let dek = self.dek.as_ref()?;
        Some((
            file.header.vault_id,
            file.header.key_generation,
            Zeroizing::new(dek.to_vec()),
        ))
    }

    // ---------- 主密码 ----------

    pub fn change_master_password(&mut self, current: &str, new: &str) -> Result<()> {
        let path = self.path.clone().ok_or(VaultError::Locked)?;
        if !Self::verify_master_password(&path, current) {
            return Err(VaultError::WrongSecret("当前主密码不正确。"));
        }

        let dek = Zeroizing::new(self.dek.as_ref().ok_or(VaultError::Locked)?.to_vec());
        let file = self.file.as_mut().ok_or(VaultError::Locked)?;

        file.header.password_salt = crypto::random_array()?;
        file.header.password_nonce = crypto::random_array()?;
        file.header.key_generation = file.header.key_generation.wrapping_add(1);

        let (m, t, p) = (file.header.m_cost_kib, file.header.t_cost, file.header.p_cost);
        let aad = file.header.password_slot_aad();
        file.password_wrapped = wrap_with_secret(
            new.as_bytes(),
            &file.header.password_salt,
            &file.header.password_nonce,
            &dek,
            &aad,
            m,
            t,
            p,
        )?;

        file.write_atomic(&path)?;
        self.unlocked_with_recovery = false;
        Ok(())
    }

    /// 恢复码流程专用:在已知数据密钥的前提下重设主密码,不校验旧密码。
    pub fn reset_master_password(&mut self, new: &str) -> Result<()> {
        let path = self.path.clone().ok_or(VaultError::Locked)?;
        let dek = Zeroizing::new(self.dek.as_ref().ok_or(VaultError::Locked)?.to_vec());
        let file = self.file.as_mut().ok_or(VaultError::Locked)?;

        file.header.password_salt = crypto::random_array()?;
        file.header.password_nonce = crypto::random_array()?;
        file.header.key_generation = file.header.key_generation.wrapping_add(1);

        let (m, t, p) = (file.header.m_cost_kib, file.header.t_cost, file.header.p_cost);
        let aad = file.header.password_slot_aad();
        file.password_wrapped = wrap_with_secret(
            new.as_bytes(),
            &file.header.password_salt,
            &file.header.password_nonce,
            &dek,
            &aad,
            m,
            t,
            p,
        )?;

        file.write_atomic(&path)?;
        self.unlocked_with_recovery = false;
        Ok(())
    }

    /// 重新生成恢复码,返回新码(旧码立即失效)。
    pub fn regenerate_recovery_code(&mut self) -> Result<String> {
        let path = self.path.clone().ok_or(VaultError::Locked)?;
        let dek = Zeroizing::new(self.dek.as_ref().ok_or(VaultError::Locked)?.to_vec());
        let file = self.file.as_mut().ok_or(VaultError::Locked)?;

        let code = recovery::generate()?;
        let normalized = recovery::normalize(&code)
            .ok_or_else(|| VaultError::Crypto("恢复码生成异常。".into()))?;

        file.header.recovery_salt = crypto::random_array()?;
        file.header.recovery_nonce = crypto::random_array()?;

        let (m, t, p) = (file.header.m_cost_kib, file.header.t_cost, file.header.p_cost);
        let aad = file.header.recovery_slot_aad();
        file.recovery_wrapped = wrap_with_secret(
            normalized.as_bytes(),
            &file.header.recovery_salt,
            &file.header.recovery_nonce,
            &dek,
            &aad,
            m,
            t,
            p,
        )?;

        file.write_atomic(&path)?;
        Ok(code)
    }

    // ---------- 条目 ----------

    pub fn active_entries(&self) -> impl Iterator<Item = &Entry> {
        self.document
            .iter()
            .flat_map(|d| d.entries.iter())
            .filter(|e| !e.is_deleted())
    }

    pub fn deleted_entries(&self) -> impl Iterator<Item = &Entry> {
        self.document
            .iter()
            .flat_map(|d| d.entries.iter())
            .filter(|e| e.is_deleted())
    }

    pub fn entry_count(&self) -> usize {
        self.active_entries().count()
    }

    pub fn deleted_count(&self) -> usize {
        self.deleted_entries().count()
    }

    pub fn known_categories(&self) -> Vec<String> {
        let Some(document) = self.document.as_ref() else {
            return Vec::new();
        };
        let mut out: Vec<String> = document
            .categories
            .iter()
            .chain(document.entries.iter().filter(|e| !e.is_deleted()).map(|e| &e.category))
            .filter(|c| !c.trim().is_empty())
            .cloned()
            .collect();
        out.sort_by(|a, b| a.cmp(b));
        out.dedup();
        out
    }

    pub fn known_tags(&self) -> Vec<String> {
        let Some(document) = self.document.as_ref() else {
            return Vec::new();
        };
        let mut out: Vec<String> = document
            .tags
            .iter()
            .chain(
                document
                    .entries
                    .iter()
                    .filter(|e| !e.is_deleted())
                    .flat_map(|e| e.tags.iter()),
            )
            .filter(|t| !t.trim().is_empty())
            .cloned()
            .collect();
        out.sort_by(|a, b| a.cmp(b));
        out.dedup();
        out
    }

    pub fn add_entry(&mut self, mut entry: Entry) -> Result<()> {
        if entry.id.is_empty() {
            entry.id = Entry::new_id()?;
        }
        entry.created = now_secs();
        entry.updated = entry.created;
        entry.deleted = None;

        let category = entry.category.clone();
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        document.entries.push(entry);
        ensure_category(document, &category);
        self.save()
    }

    pub fn update_entry(&mut self, entry: Entry) -> Result<()> {
        let category = entry.category.clone();
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        let target = document
            .entries
            .iter_mut()
            .find(|e| e.id == entry.id)
            .ok_or(VaultError::NotFound)?;

        target.title = entry.title;
        target.username = entry.username;
        target.password = entry.password;
        target.url = entry.url;
        target.notes = entry.notes;
        target.category = entry.category;
        target.tags = entry.tags;
        target.updated = now_secs();

        ensure_category(document, &category);
        self.save()
    }

    pub fn move_to_bin(&mut self, id: &str) -> Result<()> {
        let now = now_secs();
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        let target = document
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(VaultError::NotFound)?;
        target.deleted = Some(now);
        self.save()
    }

    pub fn restore_from_bin(&mut self, id: &str) -> Result<()> {
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        let target = document
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .ok_or(VaultError::NotFound)?;
        target.deleted = None;
        self.save()
    }

    pub fn purge(&mut self, id: &str) -> Result<()> {
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        document.entries.retain(|e| e.id != id);
        self.save()
    }

    pub fn empty_bin(&mut self) -> Result<()> {
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        document.entries.retain(|e| !e.is_deleted());
        self.save()
    }

    // ---------- 分类与标签(先建后用)----------

    pub fn add_category(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(VaultError::Invalid("名称不能为空。".into()));
        }

        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        if document.categories.iter().any(|c| c.eq_ignore_ascii_case(name)) {
            return Err(VaultError::Invalid("该分类已存在。".into()));
        }
        document.categories.push(name.to_string());
        self.save()
    }

    pub fn rename_category(&mut self, old: &str, new: &str) -> Result<()> {
        let new = new.trim();
        if new.is_empty() {
            return Err(VaultError::Invalid("名称不能为空。".into()));
        }
        if old == new {
            return Ok(());
        }

        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        if document.categories.iter().any(|c| c.eq_ignore_ascii_case(new)) {
            return Err(VaultError::Invalid("该分类已存在。".into()));
        }
        for category in document.categories.iter_mut() {
            if category == old {
                *category = new.to_string();
            }
        }
        for entry in document.entries.iter_mut() {
            if entry.category == old {
                entry.category = new.to_string();
            }
        }
        self.save()
    }

    /// 删除分类;用到它的条目退回「未分类」。
    pub fn remove_category(&mut self, name: &str) -> Result<()> {
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        document.categories.retain(|c| c != name);
        for entry in document.entries.iter_mut() {
            if entry.category == name {
                entry.category.clear();
            }
        }
        self.save()
    }

    pub fn add_tag(&mut self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(VaultError::Invalid("名称不能为空。".into()));
        }

        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        if document.tags.iter().any(|t| t.eq_ignore_ascii_case(name)) {
            return Err(VaultError::Invalid("该标签已存在。".into()));
        }
        document.tags.push(name.to_string());
        self.save()
    }

    pub fn rename_tag(&mut self, old: &str, new: &str) -> Result<()> {
        let new = new.trim();
        if new.is_empty() {
            return Err(VaultError::Invalid("名称不能为空。".into()));
        }
        if old == new {
            return Ok(());
        }

        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        if document.tags.iter().any(|t| t.eq_ignore_ascii_case(new)) {
            return Err(VaultError::Invalid("该标签已存在。".into()));
        }
        for tag in document.tags.iter_mut() {
            if tag == old {
                *tag = new.to_string();
            }
        }
        for entry in document.entries.iter_mut() {
            for tag in entry.tags.iter_mut() {
                if tag == old {
                    *tag = new.to_string();
                }
            }
        }
        self.save()
    }

    /// 删除标签;同时把它从所有条目上摘掉。
    pub fn remove_tag(&mut self, name: &str) -> Result<()> {
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        document.tags.retain(|t| t != name);
        for entry in document.entries.iter_mut() {
            entry.tags.retain(|t| t != name);
        }
        self.save()
    }

    /// 更新库内的设置(设置与条目一起加密保存)。
    pub fn update_settings(&mut self, settings: crate::model::Settings) -> Result<()> {
        let document = self.document.as_mut().ok_or(VaultError::Locked)?;
        document.settings = settings;
        self.save()
    }

    /// 清理回收站中超过保留期的条目,返回删除数量。
    pub fn purge_expired_bin_entries(&mut self, retention_days: i64) -> Result<usize> {
        if retention_days <= 0 {
            return Ok(0);
        }
        self.purge_bin_entries_older_than(now_secs() - retention_days * 86_400)
    }

    /// 清理删除时间早于 `cutoff`(Unix 秒)的回收站条目。
    pub fn purge_bin_entries_older_than(&mut self, cutoff: i64) -> Result<usize> {
        let removed = {
            let document = self.document.as_mut().ok_or(VaultError::Locked)?;
            let before = document.entries.len();
            document
                .entries
                .retain(|e| !matches!(e.deleted, Some(d) if d < cutoff));
            before - document.entries.len()
        };

        if removed > 0 {
            self.save()?;
        }
        Ok(removed)
    }
}

fn ensure_category(document: &mut Document, category: &str) {
    let category = category.trim();
    if category.is_empty() {
        return;
    }
    if !document.categories.iter().any(|c| c == category) {
        document.categories.push(category.to_string());
    }
}

#[allow(clippy::too_many_arguments)]
fn wrap_with_secret(
    secret: &[u8],
    salt: &[u8],
    nonce: &[u8],
    dek: &[u8],
    aad: &[u8],
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<Vec<u8>> {
    let kek = Zeroizing::new(crypto::derive_key(secret, salt, m_cost_kib, t_cost, p_cost)?);
    let wrapped = crypto::seal(&kek[..], nonce, dek, aad)?;
    if wrapped.len() != crypto::WRAPPED_KEY_LEN {
        return Err(VaultError::Crypto("包裹后的密钥长度异常。".into()));
    }
    Ok(wrapped)
}

#[allow(clippy::too_many_arguments)]
fn unwrap_with_secret(
    secret: &[u8],
    salt: &[u8],
    nonce: &[u8],
    wrapped: &[u8],
    aad: &[u8],
    m_cost_kib: u32,
    t_cost: u32,
    p_cost: u32,
) -> Result<Zeroizing<Vec<u8>>> {
    let kek = Zeroizing::new(crypto::derive_key(secret, salt, m_cost_kib, t_cost, p_cost)?);
    match crypto::open(&kek[..], nonce, wrapped, aad) {
        Ok(key) if key.len() == KEY_LEN => Ok(Zeroizing::new(key)),
        _ => Err(VaultError::WrongSecret("主密码或恢复码不正确。")),
    }
}
