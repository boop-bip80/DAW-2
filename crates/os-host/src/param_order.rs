// Copyright (C) 2026 ItsAlphaNeon
// SPDX-License-Identifier: GPL-3.0-or-later
// This file is part of Lemon Studio: https://github.com/ItsAlphaNeon/Lemon-Studio

//! Stable automation positions over a plugin's changeable parameter enumeration.

use std::collections::{BTreeMap, BTreeSet};

/// Automation targets are positions in a project; plugin APIs identify parameters by ID. Keep existing positions,
/// including missing parameters, and append new IDs. A removed ID must never redirect automation to its replacement.
#[derive(Debug, Default)]
pub struct ParamOrder {
    ids: Vec<u32>,
    indices: Vec<Option<usize>>,
    revision: u64,
}

impl ParamOrder {
    pub fn new(current: &[u32]) -> Self {
        let mut order = Self { ids: current.to_vec(), ..Self::default() };
        order.update(current);
        order
    }

    pub fn ids(&self) -> &[u32] {
        &self.ids
    }

    pub fn index(&self, position: usize) -> Option<usize> {
        self.indices.get(position).copied().flatten()
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// A host rescan also changes metadata without changing IDs or count; readers must refresh on each revision.
    pub fn update(&mut self, current: &[u32]) {
        if self.ids.is_empty() {
            self.ids = current.to_vec();
        }
        let mut known: BTreeSet<u32> = self.ids.iter().copied().collect();
        for &id in current {
            if id != u32::MAX && known.insert(id) {
                self.ids.push(id);
            }
        }
        let indices: BTreeMap<u32, usize> = current.iter().copied().enumerate()
            .filter(|(_, id)| *id != u32::MAX).map(|(i, id)| (id, i)).collect();
        self.indices = self.ids.iter().map(|id| indices.get(id).copied()).collect();
        self.revision = self.revision.wrapping_add(1);
    }

    /// Reject duplicate real IDs and unbounded metadata before changing a live map. MAX is an unreadable slot.
    pub fn valid(ids: &[u32]) -> bool {
        let mut seen = BTreeSet::new();
        ids.len() <= 65_536 && ids.iter().all(|&id| id == u32::MAX || seen.insert(id))
    }

    pub fn restore(&mut self, saved: &[u32], current: &[u32]) -> bool {
        if !Self::valid(saved) {
            return false;
        }
        self.ids = saved.to_vec();
        self.update(current);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reorder_remove_and_reintroduce_preserve_automation_positions() {
        let mut order = ParamOrder::new(&[7, 91]);
        order.update(&[91, 7]);
        assert_eq!((order.index(0), order.index(1)), (Some(1), Some(0)));
        order.update(&[91, 42]);
        assert_eq!(order.ids(), [7, 91, 42]);
        assert_eq!(order.index(0), None);
        order.update(&[42, 7, 91]);
        assert_eq!((order.index(0), order.index(1), order.index(2)), (Some(1), Some(2), Some(0)));
    }

    #[test]
    fn restore_keeps_missing_ids_and_rejects_duplicate_ids_without_mutation() {
        let mut order = ParamOrder::new(&[91, 7, 42]);
        assert!(order.restore(&[7, 12, 91], &[91, 7, 42]));
        assert_eq!(order.ids(), [7, 12, 91, 42]);
        assert_eq!(order.index(1), None);
        assert!(!order.restore(&[7, 7], &[91, 7, 42]));
        assert_eq!(order.ids(), [7, 12, 91, 42]);
        let before = order.revision();
        order.update(&[91, 7, 42]);
        assert_ne!(order.revision(), before, "a same-count metadata rescan still invalidates the UI list");
    }
}
