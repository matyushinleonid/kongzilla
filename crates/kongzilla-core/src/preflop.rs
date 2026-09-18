//! How a range hits an unknown flop.
//!
//! The statistics panel answers "what does this range have on *this* board".
//! This answers "what does this range have, full stop" - the same classification
//! averaged over every one of the 22,100 flops, which is how you judge a range
//! rather than a spot.
//!
//! Flopzilla pairs this with checkmarks: you mark the statistics you consider a
//! hit, and it reports how often the range hits at all. [`PreflopBreakdown::hit`]
//! is that number.

use std::collections::HashMap;

use crate::board::Board;
use crate::breakdown::StatRow;
use crate::cards::{CardSet, Combo};
use crate::range::Range;
use crate::stats::{stat_count, BoardContext, ClassifyOptions, StatMask};

/// A range's statistics averaged over every flop.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct PreflopBreakdown {
    /// How many flops were counted, after dead cards were removed.
    pub flops: u64,
    /// Weighted hand-flop pairs behind the percentages.
    pub total: f64,
    /// One row per visible statistic.
    pub rows: Vec<StatRow>,
    /// Share of hand-flop pairs carrying at least one checkmarked statistic.
    pub hit: f64,
    /// Every combination of statistics the pass saw, and how much weight landed
    /// on it.
    ///
    /// The per-row percentages do not depend on which statistics count as a hit;
    /// only the headline does. Keeping the shape of what was classified means
    /// that headline can be answered again for any other set of checkmarks
    /// without walking 22,100 flops a second time - which is the difference
    /// between ticking a box and waiting a second and a half to find out what
    /// ticking it did.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub profile: Vec<(StatMask, f64)>,
}

impl PreflopBreakdown {
    /// What share of the pass would count as a hit for this set of statistics.
    ///
    /// The same number [`Self::hit`] holds, asked again for a different set,
    /// and answered from the shape the pass already recorded.
    pub fn hit_for(&self, hit: StatMask) -> f64 {
        if hit.is_empty() || self.total <= 0.0 {
            return 0.0;
        }
        let weight: f64 = self
            .profile
            .iter()
            .filter(|(mask, _)| mask.intersects(hit))
            .map(|(_, weight)| weight)
            .sum();
        weight / self.total
    }

    /// Looks a row up by statistic key.
    pub fn row(&self, key: &str) -> Option<&StatRow> {
        self.rows.iter().find(|row| row.key == key)
    }
}

/// Classifies `range` against every flop that the dead cards leave available.
///
/// `hit` is the set of statistics that count as having hit; pass
/// [`StatMask::EMPTY`] when nothing is marked.
pub fn over_all_flops(
    range: &Range,
    dead: CardSet,
    options: ClassifyOptions,
    hit: StatMask,
) -> PreflopBreakdown {
    let hands: Vec<(Combo, f64)> = range
        .iter()
        .filter(|(combo, _)| !combo.mask().intersects(dead))
        .map(|(combo, weight)| (combo, f64::from(weight)))
        .collect();

    let mut totals = vec![0.0f64; stat_count()];
    let mut total = 0.0f64;
    let mut hit_weight = 0.0f64;
    let mut flops = 0u64;
    let mut profile: HashMap<StatMask, f64> = HashMap::new();

    if !hands.is_empty() {
        for flop in Board::all_flops() {
            let board_mask = flop.mask();
            if board_mask.intersects(dead) {
                continue;
            }
            flops += 1;
            let context = BoardContext::new(&flop, options);
            for (combo, weight) in &hands {
                if combo.mask().intersects(board_mask) {
                    continue;
                }
                let mask = context.classify(*combo);
                total += weight;
                *profile.entry(mask).or_insert(0.0) += weight;
                if !hit.is_empty() && mask.intersects(hit) {
                    hit_weight += weight;
                }
                for stat in mask.iter() {
                    totals[stat.index() as usize] += weight;
                }
            }
        }
    }

    let rows = options
        .visible_stats()
        .into_iter()
        .map(|stat| {
            let def = stat.def();
            let combos = totals[stat.index() as usize];
            StatRow {
                key: def.key,
                label: def.label,
                block: def.block,
                index: stat.index(),
                combos,
                fraction: if total > 0.0 { combos / total } else { 0.0 },
            }
        })
        .collect();

    PreflopBreakdown {
        flops,
        total,
        rows,
        hit: if total > 0.0 { hit_weight / total } else { 0.0 },
        profile: profile.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stats::StatId;

    #[test]
    fn made_hand_fractions_sum_to_one_over_every_flop() {
        let range = Range::parse("AKs").unwrap();
        let result = over_all_flops(
            &range,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
        );
        assert_eq!(result.flops, 22_100);
        let made: f64 = result
            .rows
            .iter()
            .filter(|row| row.block == crate::stats::StatBlock::Made)
            .map(|row| row.fraction)
            .sum();
        assert!((made - 1.0).abs() < 1e-9, "made hands summed to {made}");
    }

    #[test]
    fn a_pocket_pair_flops_a_set_about_one_time_in_eight() {
        let range = Range::parse("77").unwrap();
        let result = over_all_flops(
            &range,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
        );
        // The textbook number is 11.8% for a set, and 10.8% once the quads and
        // full houses that also contain a third seven are counted separately.
        let set = result.row("set").unwrap().fraction;
        assert!(
            (0.10..0.12).contains(&set),
            "a pocket pair flopped a set {set:.4} of the time"
        );
        // It is an overpair or better the rest of the time it is ahead.
        assert!(result.row("overpair").unwrap().fraction > 0.0);
    }

    #[test]
    fn two_overcards_flop_a_pair_about_a_third_of_the_time() {
        let range = Range::parse("AKo").unwrap();
        let result = over_all_flops(
            &range,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
        );
        let paired = result.row("top-pair").unwrap().fraction
            + result.row("middle-pair").unwrap().fraction
            + result.row("bottom-pair").unwrap().fraction
            + result.row("two-pair").unwrap().fraction
            + result.row("trips").unwrap().fraction;
        assert!(
            (0.30..0.38).contains(&paired),
            "AKo paired the board {paired:.4} of the time"
        );
    }

    #[test]
    fn the_hit_figure_follows_the_checkmarks() {
        let range = Range::parse("AKs").unwrap();
        let options = ClassifyOptions::default();
        let none = over_all_flops(&range, CardSet::EMPTY, options, StatMask::EMPTY);
        assert_eq!(none.hit, 0.0, "nothing marked means nothing hit");

        let marked: StatMask = [StatId::TOP_PAIR, StatId::FLUSH_DRAW].into_iter().collect();
        let some = over_all_flops(&range, CardSet::EMPTY, options, marked);
        assert!(some.hit > 0.0 && some.hit < 1.0, "hit was {}", some.hit);

        // Marking every made rung means the range always hits.
        let everything: StatMask = StatId::all()
            .filter(|stat| stat.def().block == crate::stats::StatBlock::Made)
            .collect();
        let all = over_all_flops(&range, CardSet::EMPTY, options, everything);
        assert!((all.hit - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dead_cards_remove_flops_and_hands() {
        let range = Range::parse("AKs").unwrap();
        let dead = CardSet::parse("As Ks").unwrap();
        let result = over_all_flops(&range, dead, ClassifyOptions::default(), StatMask::EMPTY);
        // Fifty cards leave C(50,3) flops, and the spade combination is gone.
        assert_eq!(result.flops, 19_600);
        assert!(result.total > 0.0);
    }

    #[test]
    fn an_empty_range_produces_nothing() {
        let result = over_all_flops(
            &Range::empty(),
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
        );
        assert_eq!(result.total, 0.0);
        assert_eq!(result.hit, 0.0);
        assert!(result.rows.iter().all(|row| row.fraction == 0.0));
    }
}
