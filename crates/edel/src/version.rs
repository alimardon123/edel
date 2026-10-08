//! Release versions (`2026.10.5`): how two are ordered, in the one place
//! `edel update` and Settings' Updates page both ask.

use std::cmp::Ordering;

/// Compares dotted versions number by number (`2026.10.2` > `2026.9.9`);
/// a missing part counts as 0 and a part that is not a number compares as
/// text.
pub fn compare(a: &str, b: &str) -> Ordering {
    let parts = |v: &str| -> Vec<String> { v.split('.').map(str::to_string).collect() };
    let (a, b) = (parts(a), parts(b));
    for i in 0..a.len().max(b.len()) {
        let (x, y) = (
            a.get(i).map_or("0", String::as_str),
            b.get(i).map_or("0", String::as_str),
        );
        let order = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(x), Ok(y)) => x.cmp(&y),
            _ => x.cmp(y),
        };
        if order != Ordering::Equal {
            return order;
        }
    }
    Ordering::Equal
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
}
