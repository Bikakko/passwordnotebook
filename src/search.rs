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

/// 搜索命中质量:分数越高越靠前,0 表示没命中。
///
/// 标题命中一律优先于其它字段 —— 用户输入的通常就是标题的一个片段。
/// `query` 由调用方保证已 trim 并转成小写。
pub fn match_score(entry: &Entry, query: &str) -> i32 {
    if query.is_empty() {
        // 没有搜索词时所有条目「同样命中」,不参与排序。
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
    } else if entry.url.to_lowercase().contains(query) {
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

/// 条目是否命中搜索词(供筛选用)。
pub fn matches(entry: &Entry, query: &str) -> bool {
    match_score(entry, query) > 0
}

/// 就地排序:收藏置顶 → 搜索命中质量 → 用户选的排序方式。
///
/// 收藏优先于命中质量是有意为之:收藏是用户显式表达的「常用」,不该被一次
/// 临时的搜索压下去。
pub fn sort_entries(entries: &mut [&Entry], mode: SortMode, query: &str) {
    entries.sort_by(|a, b| {
        b.favorite
            .cmp(&a.favorite)
            .then_with(|| match_score(b, query).cmp(&match_score(a, query)))
            .then_with(|| match mode {
                SortMode::Title => a.title.cmp(&b.title),
                SortMode::Category => a.category.cmp(&b.category).then(a.title.cmp(&b.title)),
                SortMode::Updated => b.updated.cmp(&a.updated),
            })
    });
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
}
