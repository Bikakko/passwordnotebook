//! 主列表的搜索命中评分与排序(纯逻辑,不依赖 Windows API)。
//!
//! 放在可移植核心里的原因:排序与搜索是用户直接感知的核心行为,而这套规则
//! (收藏置顶、标题优先、平局怎么破)一旦搬进 `src/win/main_ui.rs`,在 Linux 上
//! 就测不到了。

use crate::model::Entry;

/// 下拉框里的排序方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    /// 最近修改的在前。
    Updated,
    /// 按标题。
    Title,
    /// 按分类,同分类再按标题。
    Category,
}

impl SortMode {
    /// 下拉框下标 → 排序方式。未知值(含 -1,表示还没建好控件)按「更新时间」。
    pub fn from_combo(index: i32) -> Self {
        match index {
            1 => SortMode::Title,
            2 => SortMode::Category,
            _ => SortMode::Updated,
        }
    }
}

/// 纯 ASCII 的「忽略大小写包含」(两侧都必须已经是 ASCII;**不**处理非 ASCII)。
fn ascii_contains_fold(haystack: &str, needle: &str) -> bool {
    let n = needle.len();
    n > 0 && haystack.as_bytes().windows(n).any(|w| w.eq_ignore_ascii_case(needle.as_bytes()))
}

/// 纯 ASCII 的「忽略大小写前缀」。
fn ascii_starts_with_fold(haystack: &str, needle: &str) -> bool {
    haystack
        .as_bytes()
        .get(..needle.len())
        .is_some_and(|p| p.eq_ignore_ascii_case(needle.as_bytes()))
}

/// 大小写不敏感的包含判断。纯 ASCII 时零分配;任一侧含非 ASCII 时回落到
/// 「两边都转小写再 contains」—— Unicode 小写会改变长度,不能按字节比较。
///
/// `needle` 由调用方保证已转小写(与旧实现的契约一致)。
fn contains_fold(haystack: &str, needle: &str) -> bool {
    if haystack.is_ascii() && needle.is_ascii() {
        ascii_contains_fold(haystack, needle)
    } else {
        haystack.to_lowercase().contains(needle)
    }
}

/// 标题之外的字段:账号 40 → 网址/端点 30 → 标签 20 → 分类 15 → 备注 10 → 0。
fn score_other_fields(entry: &Entry, query: &str) -> i32 {
    if contains_fold(&entry.username, query) {
        40
    } else if contains_fold(&entry.url, query) || contains_fold(&entry.api_endpoint, query) {
        // 网址与 API 端点同级:两者都是「服务地址」,命中记同样的分。
        30
    } else if entry.tags.iter().any(|t| contains_fold(t, query)) {
        20
    } else if contains_fold(&entry.category, query) {
        15
    } else if contains_fold(&entry.notes, query) {
        10
    } else {
        0
    }
}

/// 搜索命中质量:分数越高越靠前,0 表示没命中。
///
/// 标题命中一律优先于其它字段 —— 用户输入的通常就是标题的一个片段。
/// `query` 由调用方保证已 trim 并转成小写。
///
/// 纯 ASCII(网址、英文标题这类常见数据)走免分配路径;含非 ASCII 时逐字保持
/// 旧实现的「转小写再比较」。两条路径的评分必须逐分一致,由
/// `ascii_fast_path_matches_the_old_scoring` 用对照实现钉住。
pub fn match_score(entry: &Entry, query: &str) -> i32 {
    if query.is_empty() {
        // 没有搜索词时所有条目「同样命中」,不参与排序。
        return 1;
    }

    if entry.title.is_ascii() && query.is_ascii() {
        if entry.title.eq_ignore_ascii_case(query) {
            return 100;
        }
        if ascii_starts_with_fold(&entry.title, query) {
            return 80;
        }
        if ascii_contains_fold(&entry.title, query) {
            return 60;
        }
    } else {
        let title = entry.title.to_lowercase();
        if title == query {
            return 100;
        }
        if title.starts_with(query) {
            return 80;
        }
        if title.contains(query) {
            return 60;
        }
    }

    score_other_fields(entry, query)
}

/// 条目是否命中搜索词(供筛选用)。
pub fn matches(entry: &Entry, query: &str) -> bool {
    match_score(entry, query) > 0
}

/// 主列表的筛选条件:各项之间是「与」的关系,可以任意叠加
/// (分类页签 × 标签 × 搜索词 × 只看收藏)。
///
/// 这些都是列表视图状态,不是数据本身 —— 所以 `Default` 是「什么都不筛」。
#[derive(Clone, Copy, Debug, Default)]
pub struct ListFilter<'a> {
    /// 当前分类页签对应的分类名;`None` 表示「全部」。
    pub category: Option<&'a str>,
    /// 当前选中的标签;`None` 表示「全部标签」。
    pub tag: Option<&'a str>,
    /// 搜索词(调用方负责 trim + 转小写);空串表示不按搜索词筛。
    pub query: &'a str,
    /// 「只看收藏」开关。
    pub favorites_only: bool,
}

impl ListFilter<'_> {
    /// 命中评分:`None` 表示被筛掉,`Some` 的分值与 [`match_score`] 一致。
    ///
    /// 筛选与排序各需要一次评分 —— 调用方收集 `(entry, score)` 后交给
    /// [`sort_scored`],同一份评分不必算两遍。
    pub fn score(&self, entry: &Entry) -> Option<i32> {
        if self.favorites_only && !entry.favorite {
            return None;
        }
        if self.category.is_some_and(|c| entry.category != c) {
            return None;
        }
        if self.tag.is_some_and(|t| !entry.tags.iter().any(|x| x == t)) {
            return None;
        }
        if self.query.is_empty() {
            return Some(1);
        }
        match match_score(entry, self.query) {
            0 => None,
            score => Some(score),
        }
    }

    pub fn accept(&self, entry: &Entry) -> bool {
        self.score(entry).is_some()
    }
}

/// 就地排序(评分由调用方提供):收藏置顶 → 命中质量 → 用户选的排序方式。
///
/// 与 [`sort_entries`] 同一套规则,区别只在于评分已在筛选阶段算好,这里不再重算。
pub fn sort_scored(entries: &mut [(&Entry, i32)], mode: SortMode) {
    entries.sort_by(|(a, a_score), (b, b_score)| {
        b.favorite
            .cmp(&a.favorite)
            .then_with(|| b_score.cmp(a_score))
            .then_with(|| match mode {
                SortMode::Title => a.title.cmp(&b.title),
                SortMode::Category => a.category.cmp(&b.category).then(a.title.cmp(&b.title)),
                SortMode::Updated => b.updated.cmp(&a.updated),
            })
    });
}

/// 就地排序:收藏置顶 → 搜索命中质量 → 用户选的排序方式。
///
/// 收藏优先于命中质量是有意为之:收藏是用户显式表达的「常用」,不该被一次
/// 临时的搜索压下去。
///
/// 评分**每条只算一次**再排序,而不是在比较函数里现算:`match_score` 每次调用
/// 都要把标题/账号/网址/端点/标签/分类/备注各 `to_lowercase()` 一遍(最多 7 次分配),
/// 而比较次数是 O(n log n) —— 直接在闭包里算等于把同样的字符串转换重复几十遍。
/// 「装饰-排序-写回」也保持了对相等元素的稳定顺序,与原先 `sort_by` 的结果一致。
pub fn sort_entries(entries: &mut [&Entry], mode: SortMode, query: &str) {
    let mut keyed: Vec<(&Entry, i32)> = entries
        .iter()
        .map(|e| (*e, match_score(e, query)))
        .collect();
    sort_scored(&mut keyed, mode);

    for (slot, (entry, _)) in entries.iter_mut().zip(keyed) {
        *slot = entry;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(title: &str, username: &str) -> Entry {
        Entry {
            title: title.into(),
            username: username.into(),
            ..Default::default()
        }
    }

    fn titles(entries: &[&Entry]) -> Vec<String> {
        entries.iter().map(|e| e.title.clone()).collect()
    }

    #[test]
    fn title_hits_outrank_other_fields() {
        let by_username = entry("别的", "github-user");
        let by_title = entry("github", "someone");
        assert!(match_score(&by_title, "github") > match_score(&by_username, "github"));
        assert!(match_score(&by_title, "github") > 0);
        assert!(matches(&by_username, "github"));
    }

    #[test]
    fn closer_title_matches_score_higher() {
        let exact = entry("github", "");
        let prefix = entry("github 备用", "");
        let middle = entry("我的 github 账号", "");
        assert_eq!(match_score(&exact, "github"), 100);
        assert_eq!(match_score(&prefix, "github"), 80);
        assert_eq!(match_score(&middle, "github"), 60);
        assert!(match_score(&exact, "github") > match_score(&prefix, "github"));
        assert!(match_score(&prefix, "github") > match_score(&middle, "github"));
    }

    #[test]
    fn field_priority_is_ordered() {
        let username = entry("t", "needle");
        let tag = Entry {
            title: "t".into(),
            tags: vec!["needle".into()],
            ..Default::default()
        };
        let notes = Entry {
            title: "t".into(),
            notes: "needle".into(),
            ..Default::default()
        };
        assert!(match_score(&username, "needle") > match_score(&tag, "needle"));
        assert!(match_score(&tag, "needle") > match_score(&notes, "needle"));
        assert!(match_score(&notes, "needle") > 0);
    }

    #[test]
    fn api_endpoint_ranks_like_url_and_api_key_is_not_searched() {
        let by_url = Entry {
            title: "t".into(),
            url: "https://needle.example".into(),
            ..Default::default()
        };
        let by_endpoint = Entry {
            title: "t".into(),
            api_endpoint: "https://needle.example/v1".into(),
            ..Default::default()
        };
        assert_eq!(
            match_score(&by_url, "needle"),
            match_score(&by_endpoint, "needle"),
            "端点与网址同级"
        );
        assert_eq!(match_score(&by_endpoint, "needle"), 30);

        // 密钥与密码一样不参与搜索。
        let by_key = Entry {
            title: "t".into(),
            api_key: zeroize::Zeroizing::new("sk-needle".into()),
            ..Default::default()
        };
        assert_eq!(match_score(&by_key, "needle"), 0);
        assert!(!matches(&by_key, "needle"));
    }

    #[test]
    fn empty_query_matches_everything_without_ranking() {
        let a = entry("a", "");
        let b = entry("b", "");
        assert_eq!(match_score(&a, ""), 1);
        assert!(matches(&a, ""));
        assert_eq!(match_score(&a, ""), match_score(&b, ""));
    }

    #[test]
    fn missing_query_scores_zero() {
        assert_eq!(match_score(&entry("GitHub", "alice"), "gitlab"), 0);
        assert!(!matches(&entry("GitHub", "alice"), "gitlab"));
    }

    /// 免分配快路径必须与「两边都 to_lowercase 再比较」的旧写法逐分一致。
    #[test]
    fn ascii_fast_path_matches_the_old_scoring() {
        fn old(entry: &Entry, query: &str) -> i32 {
            if query.is_empty() {
                return 1;
            }
            let hit = |s: &str| s.to_lowercase().contains(query);
            let title = entry.title.to_lowercase();
            if title == query {
                100
            } else if title.starts_with(query) {
                80
            } else if title.contains(query) {
                60
            } else if entry.username.to_lowercase().contains(query) {
                40
            } else if entry.url.to_lowercase().contains(query)
                || entry.api_endpoint.to_lowercase().contains(query)
            {
                30
            } else if entry.tags.iter().any(|t| hit(t)) {
                20
            } else if entry.category.to_lowercase().contains(query) {
                15
            } else if entry.notes.to_lowercase().contains(query) {
                10
            } else {
                0
            }
        }

        let values = [
            "GitHub", "github 备用", "GITHUB", "中文标题", "Github·中文", "", "  ", "İstanbul",
            "straße", "Straße",
        ];
        let queries = ["github", "备用", "中文", "git", "strasse", "istanbul", "z", " "];

        for title in values {
            for query in queries {
                let e = Entry {
                    title: title.into(),
                    ..Default::default()
                };
                assert_eq!(
                    match_score(&e, query),
                    old(&e, query),
                    "title={title:?} query={query:?}"
                );
            }
        }
        for username in values {
            for query in queries {
                let e = Entry {
                    username: username.into(),
                    url: "https://Example.COM/x".into(),
                    api_endpoint: "HTTPS://API.Example.com".into(),
                    category: "Ünïcode 分类".into(),
                    notes: "Notlar #1".into(),
                    tags: vec!["Önemli".into(), "urgent".into()],
                    ..Default::default()
                };
                assert_eq!(
                    match_score(&e, query),
                    old(&e, query),
                    "username={username:?} query={query:?}"
                );
            }
        }
    }

    #[test]
    fn favorites_are_pinned_above_better_matches() {
        let mut favorite = entry("zzz", "");
        favorite.favorite = true;
        let exact = entry("github", "");
        let mut items: Vec<&Entry> = vec![&exact, &favorite];
        sort_entries(&mut items, SortMode::Title, "github");
        assert_eq!(titles(&items), vec!["zzz", "github"], "收藏应压过命中质量");
    }

    #[test]
    fn title_sort_ranks_matches_then_alphabetical() {
        let beta = entry("Beta", "");
        let alpha = entry("Alpha", "");
        let beta_hit = entry("Beta github", "");
        let mut items: Vec<&Entry> = vec![&beta, &alpha, &beta_hit];
        sort_entries(&mut items, SortMode::Title, "github");
        assert_eq!(titles(&items), vec!["Beta github", "Alpha", "Beta"]);
    }

    #[test]
    fn updated_sort_puts_newest_first_and_ranks_matches() {
        let mut old = entry("Old", "");
        old.updated = 100;
        let mut new = entry("New", "");
        new.updated = 200;
        let mut hit = entry("Newest", "");
        hit.updated = 50;

        let mut items: Vec<&Entry> = vec![&old, &new, &hit];
        sort_entries(&mut items, SortMode::Updated, "");
        assert_eq!(titles(&items), vec!["New", "Old", "Newest"]);

        sort_entries(&mut items, SortMode::Updated, "new");
        assert_eq!(titles(&items)[0], "New", "完全匹配的标题应排最前");
    }

    #[test]
    fn category_sort_groups_then_sorts_by_title() {
        let mut work = entry("B", "");
        work.category = "工作".into();
        let mut work_a = entry("A", "");
        work_a.category = "工作".into();
        let mut home = entry("C", "");
        home.category = "生活".into();

        let mut items: Vec<&Entry> = vec![&work, &work_a, &home];
        sort_entries(&mut items, SortMode::Category, "");
        assert_eq!(titles(&items), vec!["A", "B", "C"]);
    }

    #[test]
    fn combo_index_maps_to_sort_mode() {
        assert_eq!(SortMode::from_combo(0), SortMode::Updated);
        assert_eq!(SortMode::from_combo(1), SortMode::Title);
        assert_eq!(SortMode::from_combo(2), SortMode::Category);
        assert_eq!(SortMode::from_combo(-1), SortMode::Updated);
        assert_eq!(SortMode::from_combo(99), SortMode::Updated);
    }

    // ---------- 列表筛选(可叠加) ----------

    fn fav(title: &str, category: &str, tags: &[&str]) -> Entry {
        Entry {
            title: title.into(),
            category: category.into(),
            tags: tags.iter().map(|t| t.to_string()).collect(),
            favorite: true,
            ..Default::default()
        }
    }

    #[test]
    fn default_filter_accepts_everything() {
        let filter = ListFilter::default();
        assert!(filter.accept(&entry("随便", "")));
        assert!(filter.accept(&fav("收藏的", "工作", &["重要"])));
    }

    #[test]
    fn favorites_only_drops_the_rest() {
        let filter = ListFilter {
            favorites_only: true,
            ..Default::default()
        };
        assert!(filter.accept(&fav("收藏的", "", &[])));
        assert!(!filter.accept(&entry("没收藏", "")));
    }

    #[test]
    fn favorites_only_composes_with_category_tag_and_query() {
        let target = fav("GitHub 主号", "工作", &["重要"]);

        let hit = ListFilter {
            favorites_only: true,
            category: Some("工作"),
            tag: Some("重要"),
            query: "github",
        };
        assert!(hit.accept(&target), "四项条件都满足应留下");

        // 任意一项不满足都要被筛掉 —— 这正是「开关能叠加」的含义。
        let wrong_category = ListFilter {
            category: Some("个人"),
            ..hit
        };
        assert!(!wrong_category.accept(&target));

        let wrong_tag = ListFilter { tag: Some("备用"), ..hit };
        assert!(!wrong_tag.accept(&target));

        let wrong_query = ListFilter { query: "gitlab", ..hit };
        assert!(!wrong_query.accept(&target));

        let not_favorite = ListFilter {
            favorites_only: false,
            ..ListFilter::default()
        };
        assert!(not_favorite.accept(&target), "关掉开关后不看收藏状态");
    }

    #[test]
    fn category_filter_matches_exactly_and_ignores_empty_category() {
        let filter = ListFilter {
            category: Some("工作"),
            ..Default::default()
        };
        let mut work = entry("有分类", "");
        work.category = "工作".into();
        let mut personal = entry("别的分类", "");
        personal.category = "个人".into();
        let uncategorized = entry("没分类", "");

        assert!(filter.accept(&work));
        assert!(!filter.accept(&personal));
        // 「未分类」的条目只在「全部」页签出现,不会被任何分类页签收进来。
        assert!(!filter.accept(&uncategorized));
    }

    /// 装饰-排序写回后,顺序必须与「比较函数里现算评分」完全一致 ——
    /// 否则用户会看到搜索/排序结果在重构前后悄悄变样。
    #[test]
    fn sort_is_identical_to_computing_the_score_in_the_comparator() {
        let entries: Vec<Entry> = (0..40)
            .map(|i| {
                let mut e = entry(&format!("标题-{:02}", i), &format!("user{i}"));
                e.favorite = i % 5 == 0;
                e.category = ["工作", "个人", ""][i % 3].into();
                e.updated = (i * 7) as i64;
                e
            })
            .collect();

        for mode in [SortMode::Updated, SortMode::Title, SortMode::Category] {
            for query in ["", "user1", "标题", "zzz"] {
                let mut actual: Vec<&Entry> = entries.iter().collect();
                sort_entries(&mut actual, mode, query);

                // 旧写法的直译,作为对照。
                let mut expected: Vec<&Entry> = entries.iter().collect();
                expected.sort_by(|a, b| {
                    b.favorite
                        .cmp(&a.favorite)
                        .then_with(|| match_score(b, query).cmp(&match_score(a, query)))
                        .then_with(|| match mode {
                            SortMode::Title => a.title.cmp(&b.title),
                            SortMode::Category => a.category.cmp(&b.category).then(a.title.cmp(&b.title)),
                            SortMode::Updated => b.updated.cmp(&a.updated),
                        })
                });

                let actual_ptrs: Vec<*const Entry> = actual.iter().map(|e| *e as *const _).collect();
                let expected_ptrs: Vec<*const Entry> = expected.iter().map(|e| *e as *const _).collect();
                assert_eq!(
                    actual_ptrs, expected_ptrs,
                    "mode={mode:?} query={query:?} 排序结果与旧写法不一致"
                );
            }
        }
    }

    /// 同上,但用**随机数据**跑,并刻意让绝大多数条目在所有比较维度上都相等 ——
    /// 全等平局正是稳定排序最容易悄悄变样的地方,固定用例不容易碰到。
    #[test]
    fn sort_matches_the_old_way_on_randomized_tied_data() {
        // 极简 LCG:随机性只用来生成用例,不值得为它引一个依赖。
        struct Rng(u64);
        impl Rng {
            fn below(&mut self, n: usize) -> usize {
                self.0 = self
                    .0
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(1442695040888963407);
                (self.0 >> 33) as usize % n
            }
        }

        // 词表刻意很小,保证大量条目在标题/分类/评分上完全打平。
        const WORDS: [&str; 5] = ["a", "A", "b", "B", "中文"];
        let mut rng = Rng(0x2026_1001);

        for round in 0..60 {
            let n = rng.below(40) + 1;
            let entries: Vec<Entry> = (0..n)
                .map(|_| Entry {
                    title: WORDS[rng.below(WORDS.len())].into(),
                    username: WORDS[rng.below(WORDS.len())].into(),
                    password: zeroize::Zeroizing::new(WORDS[rng.below(WORDS.len())].to_string()),
                    url: WORDS[rng.below(WORDS.len())].into(),
                    category: WORDS[rng.below(WORDS.len())].into(),
                    tags: vec![WORDS[rng.below(WORDS.len())].into()],
                    notes: WORDS[rng.below(WORDS.len())].into(),
                    favorite: rng.below(2) == 0,
                    created: rng.below(3) as i64,
                    updated: rng.below(3) as i64,
                    ..Default::default()
                })
                .collect();

            for mode in [SortMode::Updated, SortMode::Title, SortMode::Category] {
                for query in ["", "a", "中文", "zzz"] {
                    let mut actual: Vec<&Entry> = entries.iter().collect();
                    sort_entries(&mut actual, mode, query);

                    let mut expected: Vec<&Entry> = entries.iter().collect();
                    expected.sort_by(|a, b| {
                        b.favorite
                            .cmp(&a.favorite)
                            .then_with(|| match_score(b, query).cmp(&match_score(a, query)))
                            .then_with(|| match mode {
                                SortMode::Title => a.title.cmp(&b.title),
                                SortMode::Category => a.category.cmp(&b.category).then(a.title.cmp(&b.title)),
                                SortMode::Updated => b.updated.cmp(&a.updated),
                            })
                    });

                    let a: Vec<*const Entry> = actual.iter().map(|e| *e as *const Entry).collect();
                    let b: Vec<*const Entry> = expected.iter().map(|e| *e as *const Entry).collect();
                    assert_eq!(a, b, "round={round} n={n} mode={mode:?} query={query:?} 顺序不一致");
                }
            }
        }
    }

    /// 排序是就地写回:调用方传进来的 `&mut [&Entry]` 本身要变成新顺序。
    #[test]
    fn sort_entries_rewrites_the_callers_slice() {
        let a = entry("zebra", "");
        let b = entry("apple", "");
        let mut list: Vec<&Entry> = vec![&a, &b];
        sort_entries(&mut list, SortMode::Title, "");
        assert_eq!(titles(&list), vec!["apple".to_string(), "zebra".to_string()]);
    }
}
