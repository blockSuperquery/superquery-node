//! `BlockHeightMap<T>` — a value that changes at known heights.
//!
//! Answers "which data sources are active at block N". Entries are sparse: a value
//! set at height 100 stays in effect until the next entry, so a project with three
//! datasource activations holds three entries, not one per block.
//!
//! Backed by a `BTreeMap`, so lookups are a range query rather than a scan and
//! iteration is always in height order.
//!
//! Upstream analogue: `node-core/src/utils/blockHeightMap.ts`.

use std::collections::BTreeMap;
use std::ops::Bound;

/// A value together with the height range `[start_height, end_height]` over which
/// it applies. `end_height == None` means "open ended" (applies forever).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GetRange<T> {
    /// The value in effect over this span.
    pub value: T,
    /// First height the value applies to.
    pub start_height: u64,
    /// Last height it applies to; `None` means open-ended.
    pub end_height: Option<u64>,
}

/// Raised by [`BlockHeightMap::get`] when no entry covers the height.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryNotFoundError(
    /// The height that had no covering entry.
    pub u64,
);

impl std::fmt::Display for EntryNotFoundError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Entry not found at height {}", self.0)
    }
}

impl std::error::Error for EntryNotFoundError {}

#[derive(Debug, Clone)]
/// A sparse map from start height to the value in effect from there on.
pub struct BlockHeightMap<T> {
    map: BTreeMap<u64, T>,
}

impl<T: Clone> BlockHeightMap<T> {
    /// Build from a height-keyed map. Keys are sorted by the `BTreeMap`.
    pub fn new(initial: BTreeMap<u64, T>) -> Self {
        Self { map: initial }
    }

    /// Every entry, in ascending height order.
    pub fn get_all(&self) -> &BTreeMap<u64, T> {
        &self.map
    }

    /// Value in effect at `height`, or [`EntryNotFoundError`] if before the first entry.
    pub fn get(&self, height: u64) -> Result<&T, EntryNotFoundError> {
        self.get_details(height)
            .map(|(v, _, _)| v)
            .ok_or(EntryNotFoundError(height))
    }

    /// Like [`get`](Self::get) but returns `None` instead of erroring.
    pub fn get_safe(&self, height: u64) -> Option<&T> {
        self.get(height).ok()
    }

    /// The entry active at `height`: the greatest key `<= height`, with its
    /// `end_height` derived from the next key. Returns `(value, start, end)`.
    pub fn get_details(&self, height: u64) -> Option<(&T, u64, Option<u64>)> {
        let (start, value) = self.map.range(..=height).next_back()?;
        let end = self
            .map
            .range((Bound::Excluded(*start), Bound::Unbounded))
            .next()
            .map(|(next, _)| next - 1);
        Some((value, *start, end))
    }

    /// All entries with their ranges, in ascending order.
    pub fn get_all_with_range(&self) -> Vec<GetRange<T>> {
        let keys: Vec<u64> = self.map.keys().copied().collect();
        self.map
            .iter()
            .enumerate()
            .map(|(i, (k, v))| GetRange {
                value: v.clone(),
                start_height: *k,
                end_height: keys.get(i + 1).map(|next| next - 1),
            })
            .collect()
    }

    /// Entries relevant to indexing `[start_height, end_height]`: every key inside
    /// the range, plus the entry active *at* `start_height` (the greatest key
    /// strictly below it), provided some key `>= start_height` exists. This
    /// replicates `getWithinRange` in blockHeightMap.ts, including its edge cases.
    pub fn get_within_range(&self, start_height: u64, end_height: u64) -> BTreeMap<u64, T> {
        let mut result = BTreeMap::new();
        let mut previous_key: Option<u64> = None;

        for (key, value) in self.map.iter() {
            if let Some(prev) = previous_key {
                if prev < start_height && *key >= start_height {
                    if let Some(pv) = self.map.get(&prev) {
                        result.insert(prev, pv.clone());
                    }
                }
            }
            if *key >= start_height && *key <= end_height {
                result.insert(*key, value.clone());
            }
            previous_key = Some(*key);
        }
        result
    }

    /// Map the values, preserving heights. Port of `map()`.
    pub fn map<U: Clone>(&self, mut f: impl FnMut(&T) -> U) -> BlockHeightMap<U> {
        let mapped = self.map.iter().map(|(k, v)| (*k, f(v))).collect();
        BlockHeightMap::new(mapped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> BlockHeightMap<&'static str> {
        // heights: 1 -> "a", 10 -> "b", 20 -> "c"
        let mut m = BTreeMap::new();
        m.insert(1u64, "a");
        m.insert(10u64, "b");
        m.insert(20u64, "c");
        BlockHeightMap::new(m)
    }

    #[test]
    fn get_details_finds_active_entry_and_end() {
        let bhm = fixture();
        assert_eq!(bhm.get_details(1), Some((&"a", 1, Some(9))));
        assert_eq!(bhm.get_details(5), Some((&"a", 1, Some(9))));
        assert_eq!(bhm.get_details(10), Some((&"b", 10, Some(19))));
        assert_eq!(bhm.get_details(25), Some((&"c", 20, None)));
    }

    #[test]
    fn get_before_first_entry_errors() {
        let bhm = fixture();
        assert_eq!(bhm.get(0), Err(EntryNotFoundError(0)));
        assert_eq!(bhm.get_safe(0), None);
        assert_eq!(bhm.get(15), Ok(&"b"));
    }

    #[test]
    fn within_range_includes_active_entry_at_start() {
        let bhm = fixture();
        // Range [12, 18] — the active entry at 12 is key 10, no keys inside.
        let r = bhm.get_within_range(12, 18);
        assert_eq!(r.keys().copied().collect::<Vec<_>>(), vec![10]);

        // Range [5, 25] — active at 5 is key 1, plus 10 and 20 within range.
        let r = bhm.get_within_range(5, 25);
        assert_eq!(r.keys().copied().collect::<Vec<_>>(), vec![1, 10, 20]);
    }

    #[test]
    fn map_preserves_heights() {
        let bhm = fixture();
        let upper = bhm.map(|v| v.to_uppercase());
        assert_eq!(upper.get(5), Ok(&"A".to_string()));
        assert_eq!(upper.get(20), Ok(&"C".to_string()));
    }
}
