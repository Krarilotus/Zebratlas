//! Ranking of connections.

use atlas_core::Graph;
use atlas_core::graph::OPEN_STATUSES;

/// Study rank: exact before related, open (by status order) before closed, newer before older.
pub fn study_score(graph: &Graph, idx: u32, exact: bool) -> f64 {
    let s = graph.study(idx);
    let open = OPEN_STATUSES
        .iter()
        .position(|x| *x == s.status)
        .map_or(0.0, |i| 40.0 - i as f64 * 5.0);
    let year: f64 = s.start.get(..4).and_then(|y| y.parse().ok()).unwrap_or(1990.0);
    f64::from(u8::from(exact)) * 1000.0 + open * 10.0 + (year - 1990.0)
}
