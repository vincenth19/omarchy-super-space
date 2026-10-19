//! Short labels for video moments ("dog swimming", "beach and waves"), zero-shot: a frame's
//! vector against each line of data/labels.txt, embedded as a search query. The label vectors
//! ship in data/vectors.bin (`superspace precompute` embeds `inputs()` too).

use crate::embed::dot;
use crate::store::lookup;
use ss_core::query_text;

const LABELS: &str = include_str!("../data/labels.txt");
/// A label shows only at this cosine or above. Demo frames and photos on the Pi: right labels
/// 0.70-0.77 (median 0.73), a wrong top label 0.68. A margin over the runner-up did not help:
/// near-synonyms such as "cat" and "cat playing" are both right and score within 0.01.
const MIN: f32 = 0.70;

/// The model inputs `superspace precompute` must embed for the labels.
pub fn inputs() -> impl Iterator<Item = String> {
    LABELS.lines().map(query_text)
}

/// Each label with its int8 vector and scale.
pub struct Labels(Vec<(&'static str, Vec<i8>, f32)>);

impl Labels {
    /// The labels whose vectors `ready` (a ready-made vector file) has.
    pub fn new(ready: &[u8]) -> Labels {
        let vec = |l| lookup(ready, &query_text(l)).map(|(v, s)| (l, v.into_iter().map(|x| x as i8).collect(), s));
        Labels(LABELS.lines().filter_map(vec).collect())
    }

    /// The label for a frame's vector (int8 with its scale), when it is confident.
    pub fn best(&self, v: &[i8], scale: f32) -> Option<&'static str> {
        let cos = |(l, lv, s): &(&'static str, Vec<i8>, f32)| (*l, dot(v, lv) as f32 * s * scale);
        self.0.iter().map(cos).max_by(|a, b| a.1.total_cmp(&b.1)).filter(|b| b.1 >= MIN).map(|b| b.0)
    }
}

#[cfg(test)]
mod t {
    use super::*;
    use crate::embed::{DIM, quantize};

    #[test]
    fn labels_a_frame_only_when_confident() {
        let unit = |i: usize| (0..DIM).map(|j| f32::from(j == i)).collect::<Vec<f32>>();
        let mut recs: Vec<_> = inputs().take(2).enumerate().map(|(i, t)| crate::store::record(&t, &unit(i))).collect();
        recs.sort_by_key(|r| r.0);
        let labels = Labels::new(&recs.into_iter().flat_map(|r| r.1).collect::<Vec<u8>>());
        let best = |c: f32| {
            let (v, s) = quantize(&(0..DIM).map(|j| [c, 0.0, (1.0 - c * c).sqrt()].get(j).copied().unwrap_or(0.0)).collect::<Vec<_>>());
            labels.best(&v, s)
        };
        assert_eq!(best(0.72), LABELS.lines().next());
        assert_eq!(best(0.68), None, "a frame near no label gets none");
    }
}
