//! Condensation — build the compact folder skeleton (emergent tiers per dir).
//! Deterministic; used by `scan` to summarize the layout.

use crate::model::{Module, SkeletonEntry};
use std::collections::BTreeMap;

pub fn build_skeleton(modules: &[Module], depth_by_path: &std::collections::HashMap<String, usize>) -> Vec<SkeletonEntry> {
    let mut counts: BTreeMap<String, usize> = BTreeMap::new();
    let mut depth_sum: BTreeMap<String, usize> = BTreeMap::new();
    for m in modules {
        let segs: Vec<&str> = m.path.split('/').collect();
        let dir = match segs.len() {
            0 | 1 => "(root)".to_string(),
            2 => segs[0].to_string(),
            _ => format!("{}/{}", segs[0], segs[1]),
        };
        *counts.entry(dir.clone()).or_default() += 1;
        *depth_sum.entry(dir).or_default() += depth_by_path.get(&m.path).copied().unwrap_or(0);
    }
    let mut entries: Vec<SkeletonEntry> = counts
        .iter()
        .map(|(dir, &files)| {
            let avg = depth_sum.get(dir).copied().unwrap_or(0) as f32 / files.max(1) as f32;
            // emergent tier (L0 = most depended-upon / innermost)
            SkeletonEntry { dir: dir.clone(), role: format!("L{}", avg.round() as usize), files }
        })
        .collect();
    entries.sort_by_key(|a| std::cmp::Reverse(a.files));
    entries.truncate(25);
    entries
}
