//! Release versions (`2026.10.5`): how two are ordered, in the one place
//! `edel update` and Settings' Updates page both ask.

use std::cmp::Ordering;

/// Compares dotted versions number by number (`2026.10.2` > `2026.9.9`);
/// a missing part counts as 0 and a part that is not a number compares as
/// text.
pub fn compare(a: &str, b: &str) -> Ordering {
    // A test build's suffix (`1-rc1`) comes before the release it leads to
    // (`1`), as semver orders them (M6.17, the review of 2026-10-13).
    let (a_core, a_pre) = a.split_once('-').map_or((a, None), |(c, p)| (c, Some(p)));
    let (b_core, b_pre) = b.split_once('-').map_or((b, None), |(c, p)| (c, Some(p)));
    dotted(a_core, b_core).then_with(|| match (a_pre, b_pre) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => dotted(x, y),
    })
}

/// The value of `key` in an os-release text, without quotes. The one reader
/// of `/usr/lib/os-release` (M8.14): the release, the add-ons, the update
/// and the security facts all ask it here.
pub fn os_release_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

/// Dotted parts compared one by one, a missing part as 0; within a part,
/// runs of digits as numbers and the rest as text, so `rc10` follows `rc2`.
fn dotted(a: &str, b: &str) -> Ordering {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('.').collect(), b.split('.').collect());
    for i in 0..a.len().max(b.len()) {
        let order = natural(
            a.get(i).copied().unwrap_or("0"),
            b.get(i).copied().unwrap_or("0"),
        );
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
}

/// One part, its runs of digits compared as numbers and its other runs as
/// text.
fn natural(a: &str, b: &str) -> Ordering {
    fn runs(s: &str) -> Vec<&str> {
        let mut out = Vec::new();
        let mut start = 0;
        let mut digit = s.starts_with(|c: char| c.is_ascii_digit());
        for (i, c) in s.char_indices() {
            if c.is_ascii_digit() != digit {
                out.push(&s[start..i]);
                (start, digit) = (i, !digit);
            }
        }
        if start < s.len() {
            out.push(&s[start..]);
        }
        out
    }
    let (a, b) = (runs(a), runs(b));
    for (x, y) in a.iter().zip(&b) {
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_number_by_number() {
        assert_eq!(compare("2026.10.2", "2026.9.9"), Ordering::Greater);
        assert_eq!(compare("2026.10.4", "2026.10.5"), Ordering::Less);
        assert_eq!(compare("0.1.0", "0.1"), Ordering::Equal);
    }

    #[test]
    fn os_release_values_come_without_quotes() {
        let text = "NAME=\"Edel OS\"\nVERSION_ID=2026.10.5\n";
        assert_eq!(os_release_value(text, "NAME").as_deref(), Some("Edel OS"));
        assert_eq!(
            os_release_value(text, "VERSION_ID").as_deref(),
            Some("2026.10.5")
        );
        assert_eq!(os_release_value(text, "MISSING"), None);
    }

    #[test]
    fn a_test_build_comes_before_its_release() {
        assert_eq!(compare("1-rc1", "1"), Ordering::Less);
        assert_eq!(compare("1", "1-rc1"), Ordering::Greater);
        assert_eq!(compare("2026.10-rc2", "2026.10-rc10"), Ordering::Less);
        assert_eq!(compare("1-rc1", "1-rc1"), Ordering::Equal);
        assert_eq!(compare("1.1-rc1", "1"), Ordering::Greater);
        assert_eq!(compare("2026.10.57.1", "2026.10.57"), Ordering::Greater);
    }
}
