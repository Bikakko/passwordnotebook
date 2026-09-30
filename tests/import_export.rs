//! 导入 / 导出的集成测试:走真实文件与真实保险库,验证往返保真与去重策略。
//!
//! 与 `tests/vault.rs` 一样使用便宜版 KDF 参数(8 KiB / t=1 / p=1)以保证速度。

use std::path::PathBuf;

use zeroize::Zeroizing;

use password_notebook::export_import::{
    self, DuplicateStrategy, ExportOptions, Format, ImportOutcome,
};
use password_notebook::model::Entry;
use password_notebook::vault::VaultService;

const M: u32 = 8;
const T: u32 = 1;
const P: u32 = 1;
const PASSWORD: &str = "Correct-Horse-2024!";

struct TempDir {
    dir: PathBuf,
}

impl TempDir {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "pnb-rs-ie-{tag}-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self { dir }
    }

    fn join(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }

    /// 建一个已解锁、含若干条目的库,返回服务实例。
    fn vault_with(&self, name: &str) -> VaultService {
        let path = self.join(name);
        VaultService::create_new_with_params(&path, PASSWORD, M, T, P).unwrap();
        let mut vault = VaultService::new();
        vault.open(&path, PASSWORD).unwrap();
        vault
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn sample_entry() -> Entry {
    Entry {
        title: "带,逗号 \"引号\"".into(),
        username: "user@example.com".into(),
        password: Zeroizing::new("p,w\"d\n含换行".into()),
        url: "https://example.com/login".into(),
        category: "工作".into(),
        tags: vec!["重要".into(), "多次使用".into()],
        notes: "第一行\n第二行, 带逗号".into(),
        ..Default::default()
    }
}

fn plain_entry(title: &str, username: &str, password: &str) -> Entry {
    Entry {
        title: title.into(),
        username: username.into(),
        password: Zeroizing::new(password.into()),
        ..Default::default()
    }
}

fn assert_same_identity(a: &Entry, b: &Entry) {
    assert_eq!(a.title, b.title);
    assert_eq!(a.username, b.username);
    assert_eq!(a.password.as_str(), b.password.as_str());
    assert_eq!(a.url, b.url);
    assert_eq!(a.category, b.category);
    assert_eq!(a.tags, b.tags);
    assert_eq!(a.notes, b.notes);
}

#[test]
fn csv_export_then_import_restores_fields() {
    let tmp = TempDir::new("csv-roundtrip");
    let mut source = tmp.vault_with("source.pkk");
    source.add_entry(sample_entry()).unwrap();
    source.add_entry(plain_entry("另一个", "u2", "pw2")).unwrap();

    let file = tmp.join("export.csv");
    let count = export_import::export_to_path(
        source.document().unwrap(),
        &file,
        Format::Csv,
        ExportOptions::default(),
    )
    .unwrap();
    assert_eq!(count, 2);

    let parsed = export_import::import_from_path(&file).unwrap();
    assert_eq!(parsed.len(), 2);

    let mut target = tmp.vault_with("target.pkk");
    let outcome = target.import_entries(parsed, DuplicateStrategy::Append).unwrap();
    assert_eq!(
        outcome,
        ImportOutcome {
            added: 2,
            updated: 0,
            skipped: 0
        }
    );

    let mut restored = target
        .active_entries()
        .map(|e| e.title.clone())
        .collect::<Vec<_>>();
    restored.sort();
    let mut expected = vec!["带,逗号 \"引号\"", "另一个"];
    expected.sort();
    assert_eq!(restored, expected);

    let entry = target
        .active_entries()
        .find(|e| e.title.starts_with("带,"))
        .unwrap();
    assert_same_identity(&sample_entry(), entry);
}

#[test]
fn csv_without_passwords_does_not_wipe_existing_passwords_on_overwrite() {
    let tmp = TempDir::new("csv-nopw");
    let mut source = tmp.vault_with("source.pkk");
    source.add_entry(plain_entry("站点", "user", "super-secret")).unwrap();

    let file = tmp.join("nopw.csv");
    export_import::export_to_path(
        source.document().unwrap(),
        &file,
        Format::Csv,
        ExportOptions {
            include_passwords: false,
        },
    )
    .unwrap();

    let mut target = tmp.vault_with("target.pkk");
    target.add_entry(plain_entry("站点", "user", "原密码")).unwrap();

    let parsed = export_import::import_from_path(&file).unwrap();
    assert!(parsed[0].password.is_empty());

    let outcome = target.import_entries(parsed, DuplicateStrategy::Overwrite).unwrap();
    assert_eq!(outcome.updated, 1);

    let entry = target.active_entries().next().unwrap();
    assert_eq!(entry.password.as_str(), "原密码", "空密码不应覆盖已有密码");
}

#[test]
fn json_export_is_a_lossless_backup() {
    let tmp = TempDir::new("json-roundtrip");
    let mut source = tmp.vault_with("source.pkk");
    source.add_entry(sample_entry()).unwrap();
    // `add_entry` 会用当前时间覆盖 created/updated,所以以库内实际值为准,
    // 验证的是「导出 → 导入」这一段不丢时间戳。
    let (source_created, source_updated) = {
        let entry = source.active_entries().next().unwrap();
        (entry.created, entry.updated)
    };
    assert!(source_created > 0);

    let file = tmp.join("backup.json");
    export_import::export_to_path(
        source.document().unwrap(),
        &file,
        Format::Json,
        ExportOptions::default(),
    )
    .unwrap();

    let raw = std::fs::read_to_string(&file).unwrap();
    assert!(raw.contains(export_import::JSON_FORMAT_TAG));

    let parsed = export_import::import_from_path(&file).unwrap();
    let mut target = tmp.vault_with("target.pkk");
    target.import_entries(parsed, DuplicateStrategy::Append).unwrap();

    let restored = target.active_entries().next().unwrap();
    assert_same_identity(&sample_entry(), restored);
    assert_eq!(restored.created, source_created, "备份应保留创建时间");
    assert_eq!(restored.updated, source_updated);
    assert_eq!(target.known_categories(), vec!["工作".to_string()]);
    assert_eq!(
        target.known_tags(),
        vec!["多次使用".to_string(), "重要".to_string()]
    );
}

#[test]
fn import_respects_timestamps_from_the_file() {
    let tmp = TempDir::new("timestamps");
    let mut vault = tmp.vault_with("vault.pkk");

    let mut entry = plain_entry("有时间戳", "u", "pw");
    entry.created = 1_600_000_000;
    entry.updated = 1_600_000_500;
    vault.import_entries(vec![entry], DuplicateStrategy::Append).unwrap();

    let stored = vault.active_entries().next().unwrap();
    assert_eq!(stored.created, 1_600_000_000);
    assert_eq!(stored.updated, 1_600_000_500);
}

#[test]
fn skip_strategy_leaves_existing_entry_untouched() {
    let tmp = TempDir::new("skip");
    let mut vault = tmp.vault_with("vault.pkk");
    vault.add_entry(plain_entry("GitHub", "Me", "原密码")).unwrap();

    let incoming = vec![plain_entry("github", "me", "新密码")];
    let outcome = vault.import_entries(incoming, DuplicateStrategy::Skip).unwrap();

    assert_eq!(outcome.skipped, 1);
    assert_eq!(vault.entry_count(), 1);
    let entry = vault.active_entries().next().unwrap();
    assert_eq!(entry.password.as_str(), "原密码");
    assert_eq!(entry.title, "GitHub", "原条目不应被改写");
}

#[test]
fn overwrite_strategy_updates_existing_entry_and_keeps_id() {
    let tmp = TempDir::new("overwrite");
    let mut vault = tmp.vault_with("vault.pkk");
    let mut original = plain_entry("GitHub", "me", "旧密码");
    original.url = "https://old.example".into();
    vault.add_entry(original).unwrap();
    let original_id = vault.active_entries().next().unwrap().id.clone();
    let original_created = vault.active_entries().next().unwrap().created;

    let mut update = plain_entry("github", "ME", "新密码");
    update.url = "https://new.example".into();
    update.category = "开发".into();
    update.tags = vec!["内部".into()];

    let outcome = vault
        .import_entries(vec![update], DuplicateStrategy::Overwrite)
        .unwrap();

    assert_eq!(outcome.updated, 1);
    assert_eq!(vault.entry_count(), 1);
    let entry = vault.active_entries().next().unwrap();
    assert_eq!(entry.id, original_id, "覆盖应保留原 id");
    assert_eq!(entry.created, original_created);
    assert_eq!(entry.password.as_str(), "新密码");
    assert_eq!(entry.url, "https://new.example");
    assert_eq!(entry.category, "开发");
    assert_eq!(entry.tags, vec!["内部".to_string()]);
    assert!(vault.known_categories().contains(&"开发".to_string()));
    assert!(vault.known_tags().contains(&"内部".to_string()));
}

#[test]
fn append_strategy_always_adds_with_fresh_ids() {
    let tmp = TempDir::new("append");
    let mut vault = tmp.vault_with("vault.pkk");
    vault.add_entry(plain_entry("同一标题", "同一用户", "pw")).unwrap();

    let outcome = vault
        .import_entries(
            vec![plain_entry("同一标题", "同一用户", "pw")],
            DuplicateStrategy::Append,
        )
        .unwrap();

    assert_eq!(outcome.added, 1);
    assert_eq!(vault.entry_count(), 2);
    let ids: Vec<&str> = vault.active_entries().map(|e| e.id.as_str()).collect();
    assert_ne!(ids[0], ids[1], "重复导入也必须分配新 id");
}

/// 导入同一份 JSON 备份两次:第二次不比对就变成两份,跳过策略则一份不加。
#[test]
fn importing_own_backup_twice_is_idempotent_with_skip() {
    let tmp = TempDir::new("idempotent");
    let mut source = tmp.vault_with("source.pkk");
    source.add_entry(sample_entry()).unwrap();

    let file = tmp.join("backup.json");
    export_import::export_to_path(
        source.document().unwrap(),
        &file,
        Format::Json,
        ExportOptions::default(),
    )
    .unwrap();

    let mut target = tmp.vault_with("target.pkk");
    let first = target
        .import_entries(export_import::import_from_path(&file).unwrap(), DuplicateStrategy::Skip)
        .unwrap();
    let second = target
        .import_entries(export_import::import_from_path(&file).unwrap(), DuplicateStrategy::Skip)
        .unwrap();

    assert_eq!(first.added, 1);
    assert_eq!(second.skipped, 1);
    assert!(!second.changed());
    assert_eq!(target.entry_count(), 1);
}

#[test]
fn imported_entries_survive_reopen() {
    let tmp = TempDir::new("persist");
    let path = tmp.join("vault.pkk");
    {
        let mut vault = tmp.vault_with("vault.pkk");
        let outcome = vault
            .import_entries(
                vec![plain_entry("持久化", "u", "pw")],
                DuplicateStrategy::Append,
            )
            .unwrap();
        assert_eq!(outcome.added, 1);
    }

    let mut reopened = VaultService::new();
    reopened.open(&path, PASSWORD).unwrap();
    assert_eq!(reopened.entry_count(), 1);
    assert_eq!(reopened.active_entries().next().unwrap().title, "持久化");
}

#[test]
fn blank_rows_are_dropped() {
    let tmp = TempDir::new("blank");
    let mut vault = tmp.vault_with("vault.pkk");

    let incoming = vec![
        Entry::default(),
        plain_entry("真条目", "u", "pw"),
        Entry {
            title: "   ".into(),
            ..Default::default()
        },
    ];
    let outcome = vault.import_entries(incoming, DuplicateStrategy::Append).unwrap();
    assert_eq!(outcome.added, 1);
    assert_eq!(vault.entry_count(), 1);
}

#[test]
fn backup_file_copies_the_vault_and_stays_openable() {
    let tmp = TempDir::new("backup");
    let mut vault = tmp.vault_with("vault.pkk");
    vault.add_entry(plain_entry("备份前", "u", "pw")).unwrap();

    let backup = vault.backup_file().unwrap();
    assert!(backup.exists());
    assert_eq!(backup.extension().unwrap(), "bak");

    // 备份是加密副本:用同一个主密码能打开,且内容与备份时一致。
    let mut restored = VaultService::new();
    restored.open(&backup, PASSWORD).unwrap();
    assert_eq!(restored.entry_count(), 1);
    assert_eq!(restored.active_entries().next().unwrap().title, "备份前");
}

#[test]
fn duplicate_rows_inside_one_file_follow_the_same_strategy() {
    let tmp = TempDir::new("infiledupe");
    let file = tmp.join("dupes.csv");
    std::fs::write(
        &file,
        "标题,用户名,密码\n重复,用户,第一次\n重复,用户,第二次\n",
    )
    .unwrap();

    let entries = export_import::import_from_path(&file).unwrap();
    assert_eq!(entries.len(), 2);

    let mut vault = tmp.vault_with("vault.pkk");
    let skipped = vault.import_entries(entries, DuplicateStrategy::Skip).unwrap();
    assert_eq!(skipped.added, 1);
    assert_eq!(skipped.skipped, 1);
    assert_eq!(vault.entry_count(), 1);

    // 覆盖策略下,同一份文件里靠后的那一行最终生效。
    let entries = export_import::import_from_path(&file).unwrap();
    let overwritten = vault
        .import_entries(entries, DuplicateStrategy::Overwrite)
        .unwrap();
    assert_eq!(overwritten.updated, 2);
    assert_eq!(vault.entry_count(), 1);
    assert_eq!(
        vault.active_entries().next().unwrap().password.as_str(),
        "第二次"
    );
}

#[test]
fn import_from_path_reports_non_utf8_clearly() {
    let tmp = TempDir::new("encoding");
    let file = tmp.join("gbk.csv");
    // 典型 GBK 中文 + 非法 UTF-8 序列。
    std::fs::write(&file, [0xB1, 0xED, 0xCC, 0xE2, 0x2C, 0x61, 0x0A]).unwrap();

    let err = export_import::import_from_path(&file).unwrap_err();
    let message = err.to_string();
    assert!(message.contains("UTF-8"), "错误信息应指明编码问题:{message}");
}

#[test]
fn import_from_path_rejects_unrecognized_csv() {
    let tmp = TempDir::new("badheader");
    let file = tmp.join("bad.csv");
    std::fs::write(&file, "foo,bar\n1,2\n").unwrap();

    let err = export_import::import_from_path(&file).unwrap_err();
    assert!(err.to_string().contains("表头"));
}

#[test]
fn csv_import_of_third_party_export_lands_in_the_vault() {
    let tmp = TempDir::new("thirdparty");
    let file = tmp.join("chrome.csv");
    std::fs::write(
        &file,
        "name,url,username,password,note\n示例站点,https://example.com,me@example.com,Pa55!,备注\n",
    )
    .unwrap();

    let mut vault = tmp.vault_with("vault.pkk");
    let outcome = vault
        .import_entries(
            export_import::import_from_path(&file).unwrap(),
            DuplicateStrategy::Skip,
        )
        .unwrap();

    assert_eq!(outcome.added, 1);
    let entry = vault.active_entries().next().unwrap();
    assert_eq!(entry.title, "示例站点");
    assert_eq!(entry.url, "https://example.com");
    assert_eq!(entry.username, "me@example.com");
    assert_eq!(entry.password.as_str(), "Pa55!");
    assert_eq!(entry.notes, "备注");
}

#[test]
fn import_requires_an_unlocked_vault() {
    let locked = VaultService::new();
    let mut locked = locked;
    let err = locked
        .import_entries(vec![plain_entry("x", "y", "z")], DuplicateStrategy::Append)
        .unwrap_err();
    assert!(matches!(err, password_notebook::error::VaultError::Locked));
}
