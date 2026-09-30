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
    pub fn accept(&self, entry: &Entry) -> bool {
        if self.favorites_only && !entry.favorite {
            return false;
        }
        if self.category.is_some_and(|c| entry.category != c) {
            return false;
        }
        if self.tag.is_some_and(|t| !entry.tags.iter().any(|x| x == t)) {
            return false;
        }
        if !self.query.is_empty() && !matches(entry, self.query) {
            return false;
        }
        true
    }
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
}
