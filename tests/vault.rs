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
        let path = dir.join("vault.pkk");
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

/// 编辑器保存走 `update_entry`:它逐字段拷贝,漏一个字段就会出现
/// 「界面上保存成功、重开后少了两项」。
#[test]
fn update_entry_keeps_api_fields() {
    let tv = TempVault::new("update-api");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    let mut original = entry("服务", "开发", &[]);
    original.api_key = zeroize::Zeroizing::new("sk-old".into());
    original.api_endpoint = "https://api.old.example".into();
    vault.add_entry(original).unwrap();

    let mut target = vault.active_entries().next().unwrap().clone();
    target.api_key = zeroize::Zeroizing::new("sk-new".into());
    target.api_endpoint = "https://api.new.example/v1".into();
    vault.update_entry(target).unwrap();

    let mut reopened = VaultService::new();
    reopened.open(&tv.path, PASSWORD).unwrap();
    let stored = reopened.active_entries().next().unwrap();
    assert_eq!(stored.api_key.as_str(), "sk-new");
    assert_eq!(stored.api_endpoint, "https://api.new.example/v1");
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

/// 重设主密码失败时,不能把库留在「内存已解锁」状态。
///
/// 否则界面仍停留在锁定流程、`mode != Unlocked`,空闲锁定与锁屏事件都会跳过
/// `vault.lock()`,明文 DEK 便一直留存到进程退出。
#[test]
fn failed_password_reset_leaves_vault_locked() {
    let tv = TempVault::new("resetfail");
    let code = tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open_with_recovery_code(&tv.path, &code).unwrap();
    assert!(vault.is_unlocked());

    // 把整个目录删掉,让随后写盘必定失败。
    std::fs::remove_dir_all(&tv.dir).unwrap();

    let err = vault.reset_master_password(NEW_PASSWORD).unwrap_err();
    assert!(matches!(err, VaultError::Io(_)), "got {err:?}");
    assert!(!vault.is_unlocked(), "重设失败后不应留在解锁状态");
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

/// 改主密码时若写盘失败,内存头部必须回滚 —— 否则之后一次 save() 会把
/// 「新 salt/nonce + 旧密码槽」这种半成品持久化,主密码再也解不开。
#[test]
fn failed_password_change_rolls_back_header() {
    let tv = TempVault::new("changepwfail");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();

    // 用一个同名目录顶住临时文件路径,让 write_atomic 必定失败。
    let blocker = tv.path.with_extension("pkk.tmp");
    std::fs::create_dir(&blocker).unwrap();

    let err = vault
        .change_master_password(PASSWORD, NEW_PASSWORD)
        .unwrap_err();
    assert!(matches!(err, VaultError::Io(_)), "got {err:?}");
    assert!(vault.is_unlocked(), "改动失败也不该把库锁上");

    // 挪开障碍,把当前内存状态落盘:磁盘上应仍是旧密码可解。
    std::fs::remove_dir(&blocker).unwrap();
    vault.save().unwrap();

    assert!(VaultService::verify_master_password(&tv.path, PASSWORD));
    assert!(!VaultService::verify_master_password(&tv.path, NEW_PASSWORD));
}

/// 重新生成恢复码时若写盘失败,同样不能把半成品头部留在内存里。
#[test]
fn failed_recovery_regen_rolls_back_header() {
    let tv = TempVault::new("regenfail");
    let old = tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();

    let blocker = tv.path.with_extension("pkk.tmp");
    std::fs::create_dir(&blocker).unwrap();

    let err = vault.regenerate_recovery_code().unwrap_err();
    assert!(matches!(err, VaultError::Io(_)), "got {err:?}");

    std::fs::remove_dir(&blocker).unwrap();
    vault.save().unwrap();

    // 旧恢复码在磁盘上仍然有效。
    VaultService::new()
        .open_with_recovery_code(&tv.path, &old)
        .unwrap();
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

/// 让**下一次**落盘失败:把临时文件路径变成目录,`write_atomic` 在创建临时文件时
/// 就失败,正式文件完好无损。返回那个目录,恢复可写后删掉即可。
fn break_next_write(tv: &TempVault) -> PathBuf {
    let tmp = tv.path.with_extension("pkk.tmp");
    std::fs::create_dir(&tmp).unwrap();
    tmp
}

/// 落盘失败要留下「未落盘」标记,界面据此如实提示而不是装作没事。
#[test]
fn a_failed_save_leaves_the_unsaved_marker() {
    let tv = TempVault::new("dirty1");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    assert!(!vault.has_unsaved_changes(), "刚打开时内存与磁盘一致");

    let tmp = break_next_write(&tv);
    assert!(vault.add_entry(entry("A", "c", &[])).is_err());
    assert!(vault.has_unsaved_changes(), "落盘失败后应标记为未落盘");

    // 挡路的目录删掉,下一次成功的 save() 应当顺手清掉标记。
    std::fs::remove_dir_all(&tmp).unwrap();
    vault.save().unwrap();
    assert!(!vault.has_unsaved_changes(), "补上一次成功落盘后标记应清除");
}

/// 锁定会连未落盘的改动一起丢掉,所以标记必须跟着 document 一起清 ——
/// 否则重新解锁后会误报一条根本不存在的内容。
#[test]
fn locking_clears_the_unsaved_marker() {
    let tv = TempVault::new("dirty2");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    let tmp = break_next_write(&tv);
    assert!(vault.add_entry(entry("A", "c", &[])).is_err());
    assert!(vault.has_unsaved_changes());

    vault.lock();
    assert!(!vault.has_unsaved_changes(), "锁定后不该残留未落盘标记");

    std::fs::remove_dir_all(&tmp).unwrap();
    vault.open(&tv.path, PASSWORD).unwrap();
    assert!(!vault.has_unsaved_changes(), "重新解锁后磁盘即真相");
    assert_eq!(vault.entry_count(), 0, "没落盘的改动确实随锁定丢了");
}

/// 只重包密钥槽的三条路径直接落盘 `file.payload`,不经过 `save()`。
/// 若此刻内存里攒着未落盘的改动,那次写盘会把它们悄悄写旧内容覆盖掉 ——
/// 必须先把待写内容落盘。
#[test]
fn a_password_change_does_not_discard_pending_changes() {
    let tv = TempVault::new("dirty3");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("已落盘", "c", &[])).unwrap();

    // 让接下来的保存失败,攒下一条未落盘的改动。
    let tmp = break_next_write(&tv);
    assert!(vault.add_entry(entry("未落盘", "c", &[])).is_err());
    assert!(vault.has_unsaved_changes());

    // 挡路的目录删掉 —— 改密路径会先把待写内容落盘。
    std::fs::remove_dir_all(&tmp).unwrap();
    vault.change_master_password(PASSWORD, NEW_PASSWORD).unwrap();

    assert!(
        !vault.has_unsaved_changes(),
        "改密前应先把待写内容落盘,标记随之清除"
    );

    // 关键断言:重开磁盘文件,「未落盘」那条必须还在。
    let mut after = VaultService::new();
    after.open(&tv.path, NEW_PASSWORD).unwrap();
    let titles = set(&after.active_entries().map(|e| e.title.clone()).collect::<Vec<_>>());
    assert_eq!(
        titles,
        set(&["已落盘".to_string(), "未落盘".to_string()]),
        "改密不该把内存里待写的改动写丢"
    );
}

/// 导入是「在副本上算好再提交」,落盘成功后它连之前所有未落盘的改动一并写出,
/// 所以标记必须清掉(否则界面会一直提示有未保存内容)。
#[test]
fn a_successful_import_clears_the_unsaved_marker() {
    let tv = TempVault::new("dirty4");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();

    let tmp = break_next_write(&tv);
    assert!(vault.add_entry(entry("先失败", "c", &[])).is_err());
    assert!(vault.has_unsaved_changes());
    std::fs::remove_dir_all(&tmp).unwrap();

    let incoming = vec![entry("导入的", "c", &[])];
    vault
        .import_entries(incoming, password_notebook::export_import::DuplicateStrategy::Append)
        .unwrap();

    assert!(
        !vault.has_unsaved_changes(),
        "导入落盘成功,应把之前未落盘的改动一起写出并清标记"
    );

    let mut after = VaultService::new();
    after.open(&tv.path, PASSWORD).unwrap();
    let titles = set(&after.active_entries().map(|e| e.title.clone()).collect::<Vec<_>>());
    assert_eq!(
        titles,
        set(&["先失败".to_string(), "导入的".to_string()]),
        "导入前的未落盘改动应被一并落盘"
    );
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

// ---------- 收藏 ----------

#[test]
fn toggle_favorite_persists() {
    let tv = TempVault::new("fav");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("GitHub", "开发", &[])).unwrap();
    let id = vault.active_entries().next().unwrap().id.clone();

    assert!(!vault.active_entries().next().unwrap().favorite);
    assert!(vault.toggle_favorite(&id).unwrap());
    assert!(vault.active_entries().next().unwrap().favorite);

    // 落盘:重开之后收藏状态还在。
    let mut reopened = VaultService::new();
    reopened.open(&tv.path, PASSWORD).unwrap();
    assert!(reopened.active_entries().next().unwrap().favorite);

    assert!(!reopened.toggle_favorite(&id).unwrap());
    assert!(!reopened.active_entries().next().unwrap().favorite);
}

#[test]
fn toggle_favorite_rejects_unknown_id_and_locked_vault() {
    let tv = TempVault::new("fav-err");
    tv.create(PASSWORD);

    let mut locked = VaultService::new();
    assert!(matches!(
        locked.toggle_favorite("nope"),
        Err(VaultError::Locked)
    ));

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    assert!(matches!(
        vault.toggle_favorite("nope"),
        Err(VaultError::NotFound)
    ));
}

/// 收藏只在右键菜单里改,编辑框不提供这一项 —— 所以「编辑条目」不能顺手把
/// 收藏状态抹掉(`update_entry` 写回的是编辑器克隆出来的整条记录)。
#[test]
fn editing_an_entry_keeps_its_favorite() {
    let tv = TempVault::new("fav-edit");
    tv.create(PASSWORD);

    let mut vault = VaultService::new();
    vault.open(&tv.path, PASSWORD).unwrap();
    vault.add_entry(entry("GitHub", "开发", &[])).unwrap();

    let id = vault.active_entries().next().unwrap().id.clone();
    vault.toggle_favorite(&id).unwrap();

    // 模拟编辑器:克隆原条目,只改自己负责的字段,收藏原样带过去。
    let mut edited = vault.active_entries().next().unwrap().clone();
    assert!(edited.favorite, "编辑器拿到的应是已收藏的条目");
    edited.notes = "改过的备注".into();
    vault.update_entry(edited).unwrap();

    let after = vault.active_entries().next().unwrap();
    assert!(after.favorite, "编辑不应清掉收藏");
    assert_eq!(after.notes, "改过的备注");
}
