// SPDX-License-Identifier: GPL-3.0-or-later
// Copyright (C) 2026 Snenjih

//! [`MergeFrom`] — recursive settings-layer merge (default < user < project).
//!
//! It implements only the primitive and collection types this tree needs (no
//! `HashMap`/`IndexMap`/`Arc` — this tree only uses `BTreeMap` for
//! deterministic serialization).

use std::collections::BTreeMap;

/// Layered-merge contract: fold a higher-priority layer into `self`.
///
/// * `Option<T>`: an absent layer leaves `self` untouched; a present layer is
///   folded into an existing value, or adopted when `self` is empty.
/// * `BTreeMap<K, V>`: entries are folded per key; unseen keys are adopted.
/// * Scalars, `Vec<T>` and raw JSON values: the layer replaces `self`.
pub trait MergeFrom {
    fn merge_from(&mut self, other: &Self);

    fn merge_from_option(&mut self, other: Option<&Self>) {
        other.into_iter().for_each(|layer| self.merge_from(layer));
    }
}

/// Implements [`MergeFrom`] as plain replacement for each listed type.
macro_rules! replace_on_merge {
    ($($ty:ty),+ $(,)?) => {$(
        impl MergeFrom for $ty {
            fn merge_from(&mut self, other: &Self) {
                self.clone_from(other);
            }
        }
    )+};
}

replace_on_merge!(
    bool,
    u16,
    u32,
    u64,
    i64,
    usize,
    f32,
    f64,
    String,
    serde_json::Value,
);

impl<T: Clone> MergeFrom for Vec<T> {
    fn merge_from(&mut self, other: &Self) {
        self.clone_from(other);
    }
}

impl<T: Clone + MergeFrom> MergeFrom for Option<T> {
    fn merge_from(&mut self, other: &Self) {
        match (self.as_mut(), other) {
            (_, None) => {}
            (Some(current), Some(layer)) => current.merge_from(layer),
            (None, Some(layer)) => *self = Some(layer.clone()),
        }
    }
}

impl<K: Clone + Ord, V: Clone + MergeFrom> MergeFrom for BTreeMap<K, V> {
    fn merge_from(&mut self, other: &Self) {
        for (key, layer) in other {
            self.entry(key.clone())
                .and_modify(|current| current.merge_from(layer))
                .or_insert_with(|| layer.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn option_none_is_ignored() {
        let mut a = Some(1u32);
        a.merge_from(&None);
        assert_eq!(a, Some(1));
    }

    #[test]
    fn option_some_replaces() {
        let mut a = Some(1u32);
        a.merge_from(&Some(2));
        assert_eq!(a, Some(2));
    }

    #[test]
    fn option_none_becomes_some() {
        let mut a: Option<u32> = None;
        a.merge_from(&Some(2));
        assert_eq!(a, Some(2));
    }

    #[test]
    fn btreemap_merges_key_wise() {
        let mut a = BTreeMap::from([("a".to_string(), 1u32), ("b".to_string(), 2)]);
        let b = BTreeMap::from([("b".to_string(), 20u32), ("c".to_string(), 30)]);
        a.merge_from(&b);
        assert_eq!(a.get("a"), Some(&1));
        assert_eq!(a.get("b"), Some(&20));
        assert_eq!(a.get("c"), Some(&30));
    }

    #[test]
    fn vec_replaces_wholesale() {
        let mut a = vec![1, 2, 3];
        a.merge_from(&vec![9]);
        assert_eq!(a, vec![9]);
    }
}
