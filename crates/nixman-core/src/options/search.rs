//! Substring and prefix search over an [`OptionIndex`].
//!
//! Scoring tiers (higher wins):
//!
//! | Score | Condition                         |
//! |-------|-----------------------------------|
//! | 100   | Path is an exact match            |
//! | 80    | Path has the query as a prefix    |
//! | 60    | Path contains the query           |
//! | 40    | Description contains the query    |
//!
//! Results at the same score are ordered by path (ascending) for a stable,
//! deterministic output.

use crate::options::types::{OptionIndex, OptionMeta};

// Public API

/// Return the top `limit` options from `index` that match `query`.
///
/// Matching is case-insensitive.  When `query` is empty the first `limit`
/// options (in index order) are returned without scoring.
pub fn query<'a>(index: &'a OptionIndex, query: &str, limit: usize) -> Vec<&'a OptionMeta> {
    if query.is_empty() {
        return index.options.iter().take(limit).collect();
    }

    let q = query.to_lowercase();

    let mut scored: Vec<(&OptionMeta, u32)> = index
        .options
        .iter()
        .filter_map(|opt| {
            let path_lc = opt.path.to_lowercase();
            let desc_lc = opt.description.to_lowercase();

            let score = if path_lc == q {
                100
            } else if path_lc.starts_with(q.as_str()) {
                80
            } else if path_lc.contains(q.as_str()) {
                60
            } else if desc_lc.contains(q.as_str()) {
                40
            } else {
                return None;
            };

            Some((opt, score))
        })
        .collect();

    // Stable sort: descending score, then ascending path for tie-breaking.
    scored.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.path.cmp(&b.0.path)));

    scored.into_iter().take(limit).map(|(opt, _)| opt).collect()
}

// Tests

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::types::{OptionIndex, OptionMeta, OptionType};
    use chrono::Utc;

    fn meta(path: &str, description: &str) -> OptionMeta {
        OptionMeta {
            path: path.to_string(),
            option_type: OptionType::Bool,
            default: None,
            description: description.to_string(),
            declared_in: String::new(),
            example: None,
        }
    }

    fn index(opts: Vec<OptionMeta>) -> OptionIndex {
        OptionIndex {
            options: opts,
            flake_lock_hash: "test".to_string(),
            built_at: Utc::now(),
            nixpkgs_rev: "test".to_string(),
        }
    }

    #[test]
    fn empty_query_returns_first_n() {
        let idx = index(vec![meta("a", ""), meta("b", ""), meta("c", "")]);
        assert_eq!(query(&idx, "", 2).len(), 2);
        assert_eq!(query(&idx, "", 10).len(), 3);
    }

    #[test]
    fn exact_match_scores_highest() {
        let idx = index(vec![
            meta("nginx.enable", ""),
            meta("nginx", ""),
            meta("nginx.package", ""),
        ]);
        let results = query(&idx, "nginx", 10);
        assert_eq!(results[0].path, "nginx");
    }

    #[test]
    fn prefix_scores_higher_than_contains() {
        let idx = index(vec![
            meta("services.foo.enable", ""),
            meta("foo.bar", ""),
        ]);
        let results = query(&idx, "foo", 10);
        assert_eq!(results.len(), 2);
        // "foo.bar" starts with "foo" → score 80; "services.foo.enable" contains "foo" → score 60
        assert_eq!(results[0].path, "foo.bar");
    }

    #[test]
    fn description_match_is_included() {
        let idx = index(vec![
            meta("some.option", "Enables the nginx web server"),
        ]);
        let results = query(&idx, "nginx", 10);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "some.option");
    }

    #[test]
    fn no_match_returns_empty() {
        let idx = index(vec![meta("services.nginx.enable", "")]);
        assert!(query(&idx, "zzz_no_match", 10).is_empty());
    }

    #[test]
    fn search_is_case_insensitive() {
        let idx = index(vec![meta("Services.Nginx.Enable", "")]);
        assert_eq!(query(&idx, "services", 10).len(), 1);
        assert_eq!(query(&idx, "NGINX", 10).len(), 1);
    }

    #[test]
    fn limit_is_respected() {
        let opts: Vec<OptionMeta> = (0..20)
            .map(|i| meta(&format!("opt.{:02}", i), ""))
            .collect();
        let idx = index(opts);
        assert_eq!(query(&idx, "opt", 5).len(), 5);
    }

    #[test]
    fn tie_break_is_alphabetical_by_path() {
        let idx = index(vec![
            meta("zzz.opt", ""),
            meta("aaa.opt", ""),
            meta("mmm.opt", ""),
        ]);
        let results = query(&idx, "opt", 10);
        // All score 60 (path contains "opt") → sorted by path ascending.
        assert_eq!(results[0].path, "aaa.opt");
        assert_eq!(results[1].path, "mmm.opt");
        assert_eq!(results[2].path, "zzz.opt");
    }

    #[test]
    fn zero_limit_returns_empty() {
        let idx = index(vec![meta("services.nginx.enable", "")]);
        assert!(query(&idx, "nginx", 0).is_empty());
    }
}
