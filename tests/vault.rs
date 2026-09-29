//! 保险库核心逻辑的集成测试。
//!
//! 为保证速度,全部使用便宜版 KDF 参数(8 KiB / t=1 / p=1);
//! 参数只影响派生耗时,不影响格式与流程。

use std::path::PathBuf;

use password_notebook::error::VaultError;
use password_notebook::model::{now_secs, Entry};
use password_notebook::recovery;
use password_notebook::vault::VaultService;
use password_notebook::vaultfile::VaultFile;

const M: u32 = 8;
const T: u32 = 1;
const P: u32 = 1;
const PASSWORD: &str = "Correct-Horse-2024!";
const NEW_PASSWORD: &str = "Another-Battery-2024!";

struct TempVault {
    dir: PathBuf,
    path: PathBuf,
}

impl TempVault {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "pnb-rs-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("vault.pnb");
        Self { dir, path }
    }

    fn create(&self, password: &str) -> String {
        VaultService::create_new_with_params(&self.path, password, M, T, P).unwrap()
    }
}

impl Drop for TempVault {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn entry(title: &str, category: &str, tags: &[&str]) -> Entry {
    Entry {
        title: title.into(),
        username: format!("{title}@example.com"),
        password: zeroize::Zeroizing::new(format!("pw-{title}")),
        category: category.into(),
        tags: tags.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    }
}

#[test]
fn create_and_open() {
    let tv = TempVault::new("create");
    let code = tv.create(PASSWORD);

    assert!(VaultFile::looks_like_vault(&tv.path));
    assert_eq!(code.replace('-', "").len(), 32);
    assert!(recovery::is_valid(&code));

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    assert!(vault.is_unlocked());
    assert!(!vault.unlocked_with_recovery());
    assert_eq!(vault.entry_count(), 0);
}

#[test]
fn wrong_password_is_rejected() {
    let tv = TempVault::new("wrongpw");
    tv.create(PASSWORD);

    let err = VaultService::new().open(&tv.path, "not-the-password").unwrap_err();
    assert!(matches!(err, VaultError::WrongSecret(_)), "got {err:?}");

    assert!(!VaultService::verify_master_password(&tv.path, "nope"));
    assert!(VaultService::verify_master_password(&tv.path, PASSWORD));
}

#[test]
fn entries_survive_reopen() {
    let tv = TempVault::new("persist");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("GitHub", "开发", &["工作", "代码"])).unwrap();
    vault.add_entry(entry("邮箱", "个人", &[])).unwrap();

    assert_eq!(vault.entry_count(), 2);
    assert!(vault.known_categories().contains(&"开发".to_string()));
    assert!(vault.known_tags().contains(&"代码".to_string()));

    let mut reopened = VaultService::new();
    reopened.open(&tv.path, PASSWORD).unwrap();
    assert_eq!(reopened.entry_count(), 2);

    let mail = reopened.active_entries().find(|e| e.title == "邮箱").unwrap();
    assert_eq!(mail.password.as_str(), "pw-邮箱");
    assert_eq!(mail.category, "个人");
}

#[test]
fn update_and_delete_flow() {
    let tv = TempVault::new("crud");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("站点", "开发", &[])).unwrap();

    let mut target = vault.active_entries().next().unwrap().clone();
    target.title = "站点(已改)".into();
    target.password = zeroize::Zeroizing::new("new-password".to_string());
    vault.update_entry(target.clone()).unwrap();

    let stored = vault.active_entries().next().unwrap();
    assert_eq!(stored.title, "站点(已改)");
    assert_eq!(stored.password.as_str(), "new-password");
    assert_eq!(stored.id, target.id, "更新不应改变 id");

    vault.move_to_bin(&target.id).unwrap();
    assert_eq!(vault.entry_count(), 0);
    assert_eq!(vault.deleted_count(), 1);

    vault.restore_from_bin(&target.id).unwrap();
    assert_eq!(vault.entry_count(), 1);

    vault.purge(&target.id).unwrap();
    assert_eq!(vault.entry_count(), 0);
    assert_eq!(vault.deleted_count(), 0);
}

#[test]
fn empty_bin_keeps_active_entries() {
    let tv = TempVault::new("emptybin");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("保留", "c", &[])).unwrap();
    vault.add_entry(entry("删除", "c", &[])).unwrap();

    let doomed = vault
        .active_entries()
        .find(|e| e.title == "删除")
        .unwrap()
        .id
        .clone();
    vault.move_to_bin(&doomed).unwrap();
    vault.empty_bin().unwrap();

    assert_eq!(vault.entry_count(), 1);
    assert_eq!(vault.deleted_count(), 0);
    assert_eq!(vault.active_entries().next().unwrap().title, "保留");
}

#[test]
fn expired_bin_entries_are_purged() {
    let tv = TempVault::new("purge");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("A", "c", &[])).unwrap();
    let id = vault.active_entries().next().unwrap().id.clone();
    vault.move_to_bin(&id).unwrap();

    assert_eq!(vault.purge_expired_bin_entries(30).unwrap(), 0, "刚删除不应被清理");
    assert_eq!(vault.purge_expired_bin_entries(0).unwrap(), 0, "关闭保留期时不清空");

    // 保留期内的不会被清掉。
    assert_eq!(vault.purge_bin_entries_older_than(now_secs() - 60).unwrap(), 0);

    // 用一个「未来」的截止时间,模拟条目已过期。
    assert_eq!(vault.purge_bin_entries_older_than(now_secs() + 1).unwrap(), 1);
    assert_eq!(vault.deleted_count(), 0);
}

#[test]
fn recovery_code_unlocks_and_resets_password() {
    let tv = TempVault::new("recovery");
    let code = tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open_with_recovery_code(&tv.path, &code).unwrap();
    assert!(vault.unlocked_with_recovery());
    assert!(vault.is_unlocked());

    vault.reset_master_password(NEW_PASSWORD).unwrap();
    vault.lock();
    assert!(!vault.is_unlocked());

    let mut after = VaultService::new();
    after.open(&tv.path, NEW_PASSWORD).unwrap();
    assert!(!after.unlocked_with_recovery());
    assert!(!VaultService::verify_master_password(&tv.path, PASSWORD));
}

#[test]
fn recovery_code_accepts_loose_input() {
    let tv = TempVault::new("loose");
    let code = tv.create(PASSWORD);

    let messy = format!("  {}  ", code.replace('-', " ").to_lowercase());
    let mut vault = VaultService::new();
    vault.open_with_recovery_code(&tv.path, &messy).unwrap();
    assert!(vault.is_unlocked());
}

#[test]
fn bad_recovery_code_is_rejected() {
    let tv = TempVault::new("badrecovery");
    tv.create(PASSWORD);

    let err = VaultService::new()
        .open_with_recovery_code(&tv.path, "AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA-AAAA")
        .unwrap_err();
    assert!(matches!(err, VaultError::WrongSecret(_)));

    let err = VaultService::new()
        .open_with_recovery_code(&tv.path, "too-short")
        .unwrap_err();
    assert!(matches!(err, VaultError::WrongSecret(_)));
}

#[test]
fn change_master_password_keeps_recovery_code_and_data() {
    let tv = TempVault::new("changepw");
    let code = tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("数据", "c", &[])).unwrap();
    vault.change_master_password(PASSWORD, NEW_PASSWORD).unwrap();

    assert!(VaultService::verify_master_password(&tv.path, NEW_PASSWORD));
    assert!(!VaultService::verify_master_password(&tv.path, PASSWORD));

    // 改密码不应该影响恢复码槽与载荷。
    let mut via_recovery = VaultService::new();
    via_recovery.open_with_recovery_code(&tv.path, &code).unwrap();
    assert_eq!(via_recovery.entry_count(), 1);

    let mut fresh = VaultService::new();
    fresh.open(&tv.path, NEW_PASSWORD).unwrap();
    assert_eq!(fresh.entry_count(), 1);
    assert_eq!(fresh.active_entries().next().unwrap().title, "数据");
}

#[test]
fn change_master_password_requires_correct_current() {
    let tv = TempVault::new("changepwbad");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    let err = vault.change_master_password("wrong-current", NEW_PASSWORD).unwrap_err();
    assert!(matches!(err, VaultError::WrongSecret(_)));
    assert!(VaultService::verify_master_password(&tv.path, PASSWORD));
}

#[test]
fn regenerate_recovery_code_invalidates_old_one() {
    let tv = TempVault::new("regen");
    let old = tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    let new = vault.regenerate_recovery_code().unwrap();
    assert_ne!(old, new);

    assert!(VaultService::new().open_with_recovery_code(&tv.path, &old).is_err());
    assert!(VaultService::new().open_with_recovery_code(&tv.path, &new).is_ok());
}

#[test]
fn cached_key_unlock() {
    let tv = TempVault::new("cached");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    let (_, _, dek) = vault.quick_unlock_material().unwrap();

    let mut cached = VaultService::new();
    cached.open_with_cached_key(&tv.path, dek.clone()).unwrap();
    assert!(cached.is_unlocked());

    let mut forged = VaultService::new();
    let err = forged
        .open_with_cached_key(&tv.path, zeroize::Zeroizing::new(vec![0u8; 32]))
        .unwrap_err();
    assert!(matches!(err, VaultError::WrongSecret(_)));
}

#[test]
fn tampered_payload_is_detected() {
    let tv = TempVault::new("tamperpayload");
    tv.create(PASSWORD);

    let mut bytes = std::fs::read(&tv.path).unwrap();
    let last = bytes.len() - 1;
    bytes[last] ^= 0xFF;
    std::fs::write(&tv.path, &bytes).unwrap();

    assert!(VaultService::new().open(&tv.path, PASSWORD).is_err());
}

#[test]
fn tampered_header_is_detected() {
    let tv = TempVault::new("tamperheader");
    tv.create(PASSWORD);

    let mut bytes = std::fs::read(&tv.path).unwrap();
    bytes[20] ^= 0xFF; // vault_id 区域,会改变所有 AAD
    std::fs::write(&tv.path, &bytes).unwrap();

    assert!(VaultService::new().open(&tv.path, PASSWORD).is_err());
}

#[test]
fn locked_service_refuses_operations() {
    let tv = TempVault::new("locked");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    assert!(matches!(vault.save().unwrap_err(), VaultError::Locked));
    assert!(matches!(
        vault.add_entry(entry("x", "c", &[])).unwrap_err(),
        VaultError::Locked
    ));

    vault.open(&tv.path, PASSWORD).unwrap();
    vault.lock();
    assert!(!vault.is_unlocked());
    assert!(vault.document().is_none());
    assert!(matches!(vault.save().unwrap_err(), VaultError::Locked));
}

/// 比较集合(顺序无关)。`known_*` 返回是排好序的,但中文的排序顺序不好预判。
fn set<T: AsRef<str>>(names: &[T]) -> std::collections::BTreeSet<String> {
    names.iter().map(|s| s.as_ref().to_string()).collect()
}

#[test]
fn taxonomy_is_managed_separately_from_entries() {
    let tv = TempVault::new("taxonomy");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();

    // 分类:先建,再让条目引用
    vault.add_category("开发").unwrap();
    vault.add_category("个人").unwrap();
    assert_eq!(set(&vault.known_categories()), set(&["开发", "个人"]));
    assert!(vault.add_category("开发").is_err(), "重复分类应被拒绝");

    vault.add_tag("工作").unwrap();
    vault.add_tag("代码").unwrap();
    assert_eq!(set(&vault.known_tags()), set(&["工作", "代码"]));

    let mut item = entry("站点", "开发", &["工作"]);
    item.id = Entry::new_id().unwrap();
    vault.add_entry(item).unwrap();

    // 重命名:条目上的引用要跟着改
    vault.rename_category("开发", "研发").unwrap();
    assert_eq!(set(&vault.known_categories()), set(&["个人", "研发"]));
    assert_eq!(vault.active_entries().next().unwrap().category, "研发");

    vault.rename_tag("工作", "上班").unwrap();
    assert_eq!(set(&vault.known_tags()), set(&["代码", "上班"]));
    assert_eq!(vault.active_entries().next().unwrap().tags, vec!["上班"]);

    // 删除:条目上的引用要清掉,条目本身保留
    vault.remove_tag("上班").unwrap();
    assert_eq!(set(&vault.known_tags()), set(&["代码"]));
    let only = vault.active_entries().next().unwrap();
    assert!(only.tags.is_empty());
    assert_eq!(only.title, "站点", "删标签不应动到条目本身");

    vault.remove_category("研发").unwrap();
    assert!(vault.active_entries().next().unwrap().category.is_empty());
    assert_eq!(vault.entry_count(), 1, "删分类不应删掉条目");

    // 重新打开后仍然是这套
    let mut reopened = VaultService::new();
    reopened.open(&tv.path, PASSWORD).unwrap();
    assert_eq!(set(&reopened.known_tags()), set(&["代码"]));
    // 「个人」还在(只删了「研发」)
    assert_eq!(set(&reopened.known_categories()), set(&["个人"]));
}

/// 文件格式敏感:Entry.password 换成 Zeroizing<String> 后,JSON 表示
/// 必须仍然是普通字符串,否则已存在的 .pkk 会读不出来。
#[test]
fn entry_password_stays_a_plain_json_string() {
    let entry = Entry {
        password: zeroize::Zeroizing::new("pw-x".to_string()),
        ..Default::default()
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert!(
        json.contains("\"password\":\"pw-x\""),
        "password 的 JSON 表示被改动了:{json}"
    );

    // 反向:旧的 JSON(普通字符串)必须还能读进来。
    let back: Entry = serde_json::from_str(&json).unwrap();
    assert_eq!(back.password.as_str(), "pw-x");
}
