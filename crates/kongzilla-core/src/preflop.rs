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
use crate::cards::{CardSet, Combo, NUM_COMBOS};
use crate::flops::FlopFilter;
use crate::range::Range;
use crate::stats::{stat_count, BoardContext, ClassifyOptions, StatBlock, StatId, StatMask, ORDER};

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
    /// How much of each hand's weight landed in each tier of the made ladder,
    /// as `combo * TIERS.len() + tier`.
    ///
    /// The pie next door divides the range into these four; this says which
    /// hands are behind each division, which is what lets a wedge be read hand
    /// by hand rather than as one block of colour. Kept out of the JSON: it is
    /// five thousand numbers that only the pie asks for, and it asks for them
    /// once a pass rather than once a redraw.
    #[cfg_attr(feature = "serde", serde(skip))]
    pub tiers: Vec<f32>,
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

/// Where the made ladder is cut for a reading of the whole range.
///
/// Seventeen rungs is a table rather than a pie, and half of them are slivers
/// nobody can compare. These four are the cuts a preflop decision turns on, and
/// each is a *run* of the ladder starting at the named rung - so a rung added
/// between two of them falls in the tier it was added to rather than going
/// missing.
pub const LADDER_TIERS: [StatId; 4] = [
    StatId::STRAIGHT_FLUSH,
    StatId::OVERPAIR,
    StatId::PP_BELOW_TOP_CARD,
    StatId::ACE_HIGH,
];

/// Which tier a classified hand belongs to.
///
/// Exactly one rung of the made block is set on any hand - a flushdraw with no
/// pair is no pair - so the strongest rung the mask carries is the hand it has.
fn tier_of(mask: StatMask) -> usize {
    let mut tier = LADDER_TIERS.len() - 1;
    let mut at = 0;
    for stat in ORDER {
        if stat.def().block != StatBlock::Made {
            break;
        }
        if at + 1 < LADDER_TIERS.len() && LADDER_TIERS[at + 1] == stat {
            at += 1;
        }
        if mask.has(stat) {
            tier = at;
            break;
        }
    }
    tier
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

/// Classifies `range` against the flops `off_the_deck` and `filter` leave.
///
/// Two card sets, because they answer two questions. A flop cannot use a card
/// anybody is holding, this range's owner included - that is `off_the_deck`.
/// A hand of this range cannot use a card somebody *else* is holding, but its
/// own cards are the whole point of it - that is `held_elsewhere`. Passing one
/// set for both is how a seat holding a hand came to have no hands at all.
///
/// `hit` is the set of statistics that count as having hit; pass
/// [`StatMask::EMPTY`] when nothing is marked. An empty `filter` is every flop,
/// which is the ordinary pass.
pub fn over_flops(
    range: &Range,
    off_the_deck: CardSet,
    held_elsewhere: CardSet,
    options: ClassifyOptions,
    hit: StatMask,
    filter: FlopFilter,
) -> PreflopBreakdown {
    let hands: Vec<(Combo, f64)> = range
        .iter()
        .filter(|(combo, _)| !combo.mask().intersects(held_elsewhere))
        .map(|(combo, weight)| (combo, f64::from(weight)))
        .collect();

    let mut totals = vec![0.0f64; stat_count()];
    let mut total = 0.0f64;
    let mut hit_weight = 0.0f64;
    let mut flops = 0u64;
    let mut profile: HashMap<StatMask, f64> = HashMap::new();
    let mut tiers = vec![0.0f32; NUM_COMBOS * LADDER_TIERS.len()];

    if !hands.is_empty() {
        for flop in Board::all_flops() {
            let board_mask = flop.mask();
            if board_mask.intersects(off_the_deck) || !filter.matches(&flop) {
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
                let at = combo.index() as usize * LADDER_TIERS.len() + tier_of(mask);
                tiers[at] += *weight as f32;
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
        tiers,
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
        let result = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
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
    fn the_tiers_say_which_hands_are_behind_them() {
        let range = Range::parse("AA, 72o").unwrap();
        let result = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
        );

        // Every hand-flop pair the pass counted is in exactly one tier, so the
        // four of them together are the whole pass.
        let summed: f64 = result.tiers.iter().map(|weight| f64::from(*weight)).sum();
        assert!(
            (summed - result.total).abs() / result.total < 1e-3,
            "tiers held {summed} of {}",
            result.total
        );

        // And each tier holds what the rows it gathers hold. The made rows are
        // exclusive, so a tier is the sum of its run of them.
        let made: Vec<&StatRow> = result
            .rows
            .iter()
            .filter(|row| row.block == StatBlock::Made)
            .collect();
        let mut by_rows = [0.0f64; LADDER_TIERS.len()];
        let mut at = 0;
        for row in made {
            if at + 1 < LADDER_TIERS.len() && LADDER_TIERS[at + 1].def().key == row.key {
                at += 1;
            }
            by_rows[at] += row.combos;
        }
        for (tier, rows) in by_rows.iter().enumerate() {
            let held: f64 = range
                .iter()
                .map(|(combo, _)| {
                    f64::from(result.tiers[combo.index() as usize * LADDER_TIERS.len() + tier])
                })
                .sum();
            assert!(
                (held - rows).abs() / result.total < 1e-3,
                "tier {tier} held {held}, its rows {rows}"
            );
        }

        // The hands are told apart, which is the whole point of counting them
        // one at a time: aces are an overpair on most flops, and seven-deuce is
        // no pair on most flops.
        let share = |combo: Combo, tier: usize| {
            let at = combo.index() as usize * LADDER_TIERS.len();
            let held: f32 = result.tiers[at..at + LADDER_TIERS.len()].iter().sum();
            f64::from(result.tiers[at + tier] / held)
        };
        let aces = Combo::parse("AhAs").expect("a hand");
        let rags = Combo::parse("7h2s").expect("a hand");
        assert!(share(aces, 1) > 0.7, "aces: {}", share(aces, 1));
        assert!(share(rags, 3) > 0.6, "seven-deuce: {}", share(rags, 3));
    }

    #[test]
    fn a_pocket_pair_flops_a_set_about_one_time_in_eight() {
        let range = Range::parse("77").unwrap();
        let result = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
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
        let result = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
        );
        let paired = result.row("top-pair").unwrap().fraction
            + result.row("second-pair").unwrap().fraction
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
        let none = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            options,
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
        );
        assert_eq!(none.hit, 0.0, "nothing marked means nothing hit");

        let marked: StatMask = [StatId::TOP_PAIR, StatId::FLUSH_DRAW].into_iter().collect();
        let some = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            options,
            marked,
            FlopFilter::EVERYTHING,
        );
        assert!(some.hit > 0.0 && some.hit < 1.0, "hit was {}", some.hit);

        // Marking every made rung means the range always hits.
        let everything: StatMask = StatId::all()
            .filter(|stat| stat.def().block == crate::stats::StatBlock::Made)
            .collect();
        let all = over_flops(
            &range,
            CardSet::EMPTY,
            CardSet::EMPTY,
            options,
            everything,
            FlopFilter::EVERYTHING,
        );
        assert!((all.hit - 1.0).abs() < 1e-9);
    }

    #[test]
    fn dead_cards_remove_flops_and_hands() {
        let range = Range::parse("AKs").unwrap();
        let dead = CardSet::parse("As Ks").unwrap();
        let result = over_flops(
            &range,
            dead,
            dead,
            ClassifyOptions::default(),
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
        );
        // Fifty cards leave C(50,3) flops, and the spade combination is gone.
        assert_eq!(result.flops, 19_600);
        assert!(result.total > 0.0);
    }

    #[test]
    fn an_empty_range_produces_nothing() {
        let result = over_flops(
            &Range::empty(),
            CardSet::EMPTY,
            CardSet::EMPTY,
            ClassifyOptions::default(),
            StatMask::EMPTY,
            FlopFilter::EVERYTHING,
        );
        assert_eq!(result.total, 0.0);
        assert_eq!(result.hit, 0.0);
        assert!(result.rows.iter().all(|row| row.fraction == 0.0));
    }
}
