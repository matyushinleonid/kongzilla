//! The statistics panel's numbers.

use crate::range::Range;
use crate::stats::{ClassifyOptions, ComboStats, StatBlock, StatId, StatMask};

/// Whether a row reports its own share or its share plus everything above it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "kebab-case"))]
pub enum BreakdownMode {
    /// How often the range holds exactly this.
    #[default]
    Absolute,
    /// How often the range holds this hand or better.
    ///
    /// Only the made-hand block is a ladder, so only that block accumulates.
    Cumulative,
}

/// One row of the statistics panel.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct StatRow {
    /// The statistic's stable key.
    pub key: &'static str,
    /// The name shown in the panel.
    pub label: &'static str,
    /// Which coloured block the row belongs to.
    pub block: StatBlock,
    /// The statistic's registry index.
    pub index: u8,
    /// Weighted combos that carry the statistic.
    pub combos: f64,
    /// Share of the range, in `0.0..=1.0`.
    pub fraction: f64,
}

/// Every row of the statistics panel plus the totals beneath it.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Breakdown {
    /// Weighted combos in the range that are playable against this board.
    pub total_combos: f64,
    /// The rows, in registry order.
    pub rows: Vec<StatRow>,
    /// Which mode produced the fractions.
    pub mode: BreakdownMode,
}

impl Breakdown {
    /// Looks a row up by statistic.
    pub fn row(&self, stat: StatId) -> Option<&StatRow> {
        self.rows.iter().find(|r| r.index == stat.index())
    }
}

/// Builds the statistics panel for `range` against a classified board.
pub fn breakdown(
    range: &Range,
    stats: &ComboStats,
    options: ClassifyOptions,
    mode: BreakdownMode,
) -> Breakdown {
    breakdown_within(range, stats, options, mode, StatMask::EMPTY)
}

/// Builds the statistics panel restricted to combos that carry every statistic in
/// `within`.
///
/// This is what hovering a statistic does: the panel re-filters to that statistic
/// so the overlap with every other one can be read straight off the numbers.
pub fn breakdown_within(
    range: &Range,
    stats: &ComboStats,
    options: ClassifyOptions,
    mode: BreakdownMode,
    within: StatMask,
) -> Breakdown {
    let visible = options.visible_stats();
    let mut totals = vec![0.0f64; crate::stats::stat_count()];
    let mut total = 0.0f64;

    for combo in stats.live().iter() {
        let weight = f64::from(range.get(combo));
        if weight <= 0.0 {
            continue;
        }
        let mask = stats.mask(combo);
        if !within.is_empty() && mask.intersection(within) != within {
            continue;
        }
        total += weight;
        for stat in mask.iter() {
            totals[stat.index() as usize] += weight;
        }
    }

    let mut rows = Vec::with_capacity(visible.len());
    let mut running = 0.0f64;
    for stat in visible {
        let def = stat.def();
        let combos = totals[stat.index() as usize];
        let value = match (mode, def.block) {
            (BreakdownMode::Cumulative, StatBlock::Made) => {
                running += combos;
                running
            }
            _ => combos,
        };
        rows.push(StatRow {
            key: def.key,
            label: def.label,
            block: def.block,
            index: stat.index(),
            combos: value,
            fraction: if total > 0.0 { value / total } else { 0.0 },
        });
    }

    Breakdown {
        total_combos: total,
        rows,
        mode,
    }
}

/// How often one statistic comes with another.
///
/// Cell `(row, column)` is `P(column | row)`: of the hands that hold the row's
/// statistic, the share that also hold the column's. This is the whole hover
/// interaction laid out at once - Flopzilla keeps it as a hidden panel below the
/// main window, and it is the fastest way to see that top pair is a flushdraw a
/// tenth of the time on this board.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct OverlapMatrix {
    /// The statistics on both axes, in registry order.
    pub stats: Vec<u8>,
    /// Their labels, in the same order.
    pub labels: Vec<&'static str>,
    /// Weighted combos holding each statistic.
    pub totals: Vec<f64>,
    /// `rows[row][column]` is `P(column | row)`, in `0.0..=1.0`.
    pub rows: Vec<Vec<f64>>,
}

/// Builds the overlap matrix for `range` against a classified board.
pub fn overlap(range: &Range, stats: &ComboStats, options: ClassifyOptions) -> OverlapMatrix {
    let visible = options.visible_stats();
    let index: Vec<usize> = {
        let mut lookup = vec![usize::MAX; crate::stats::stat_count()];
        for (position, stat) in visible.iter().enumerate() {
            lookup[stat.index() as usize] = position;
        }
        lookup
    };

    let size = visible.len();
    let mut totals = vec![0.0f64; size];
    let mut joint = vec![vec![0.0f64; size]; size];

    for combo in stats.live().iter() {
        let weight = f64::from(range.get(combo));
        if weight <= 0.0 {
            continue;
        }
        // A hand carries only a handful of statistics, so the pairwise loop is
        // over five-ish entries rather than the whole registry.
        let held: Vec<usize> = stats
            .mask(combo)
            .iter()
            .map(|stat| index[stat.index() as usize])
            .filter(|position| *position != usize::MAX)
            .collect();
        for &row in &held {
            totals[row] += weight;
            for &column in &held {
                joint[row][column] += weight;
            }
        }
    }

    let rows = joint
        .into_iter()
        .enumerate()
        .map(|(row, cells)| {
            let total = totals[row];
            cells
                .into_iter()
                .map(|value| if total > 0.0 { value / total } else { 0.0 })
                .collect()
        })
        .collect();

    OverlapMatrix {
        stats: visible.iter().map(|stat| stat.index()).collect(),
        labels: visible.iter().map(|stat| stat.def().label).collect(),
        totals,
        rows,
    }
}

/// How much of each matrix cell carries `mask`, as a share of what the cell holds.
///
/// Dividing by the cell's own weight rather than by its combo count is what makes
/// the highlight readable: one flushdraw among twelve offsuit combinations is a
/// twelfth of the cell but *all* of what that cell contributes, and the eye needs
/// to see it. An empty mask lights everything the range holds.
pub fn highlight(
    range: &Range,
    stats: &ComboStats,
    mask: StatMask,
) -> [f32; crate::cards::NUM_CLASSES] {
    let mut matched = [0.0f32; crate::cards::NUM_CLASSES];
    let mut held = [0.0f32; crate::cards::NUM_CLASSES];
    for combo in stats.live().iter() {
        let weight = range.get(combo);
        if weight <= 0.0 {
            continue;
        }
        let class = combo.class().index() as usize;
        held[class] += weight;
        if mask.is_empty() || stats.mask(combo).intersection(mask) == mask {
            matched[class] += weight;
        }
    }
    let mut share = [0.0f32; crate::cards::NUM_CLASSES];
    for index in 0..crate::cards::NUM_CLASSES {
        if held[index] > 0.0 {
            share[index] = matched[index] / held[index];
        }
    }
    share
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::cards::CardSet;

    fn setup(range: &str, board: &str) -> (Range, ComboStats) {
        let range = Range::parse(range).unwrap();
        let board = Board::parse(board).unwrap();
        let stats = ComboStats::build(&board, CardSet::EMPTY, ClassifyOptions::default());
        (range, stats)
    }

    #[test]
    fn made_hand_fractions_sum_to_one() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kc Qh Jh");
        let panel = breakdown(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Absolute,
        );
        let made: f64 = panel
            .rows
            .iter()
            .filter(|r| r.block == StatBlock::Made)
            .map(|r| r.fraction)
            .sum();
        assert!((made - 1.0).abs() < 1e-9, "made fractions summed to {made}");
        assert!(panel.total_combos > 0.0);
    }

    #[test]
    fn cumulative_mode_accumulates_the_made_ladder() {
        let (range, stats) = setup("22+,A2s+", "Kc Qh Jh");
        let absolute = breakdown(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Absolute,
        );
        let cumulative = breakdown(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Cumulative,
        );
        let made: Vec<&StatRow> = cumulative
            .rows
            .iter()
            .filter(|r| r.block == StatBlock::Made)
            .collect();
        for pair in made.windows(2) {
            assert!(
                pair[1].combos >= pair[0].combos - 1e-9,
                "ladder went backwards"
            );
        }
        assert!((made.last().unwrap().fraction - 1.0).abs() < 1e-9);
        // Draw rows are not a ladder and stay absolute.
        let key = "flushdraw";
        assert_eq!(
            absolute.rows.iter().find(|r| r.key == key).unwrap().combos,
            cumulative
                .rows
                .iter()
                .find(|r| r.key == key)
                .unwrap()
                .combos
        );
    }

    #[test]
    fn hovering_restricts_the_panel() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kc Qh Jh");
        let all = breakdown(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Absolute,
        );
        let within = breakdown_within(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Absolute,
            StatId::TOP_PAIR.mask(),
        );
        let top_pair = all.row(StatId::TOP_PAIR).unwrap().combos;
        assert!((within.total_combos - top_pair).abs() < 1e-9);
        assert!((within.row(StatId::TOP_PAIR).unwrap().fraction - 1.0).abs() < 1e-9);
        let overlap = within.row(StatId::FLUSH_DRAW).unwrap().fraction;
        assert!((0.0..=1.0).contains(&overlap));
    }

    #[test]
    fn optional_stats_appear_only_when_enabled() {
        let (range, stats) = setup("A2s+", "Kh 7h 2c");
        let off = breakdown(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Absolute,
        );
        assert!(off.row(StatId::BACKDOOR_FLUSH_DRAW_1_HIGH).is_none());
        let options = ClassifyOptions {
            one_card_backdoor_flushdraw: true,
        };
        let stats = ComboStats::build(&Board::parse("Kh 7h 2c").unwrap(), CardSet::EMPTY, options);
        let on = breakdown(&range, &stats, options, BreakdownMode::Absolute);
        assert!(on.row(StatId::BACKDOOR_FLUSH_DRAW_1_HIGH).is_some());
    }

    #[test]
    fn the_overlap_matrix_reads_as_conditional_probability() {
        let (range, stats) = setup("22+,A2s+,KJs+,AJo+", "Kc Qh Jh");
        let matrix = overlap(&range, &stats, ClassifyOptions::default());
        let panel = breakdown(
            &range,
            &stats,
            ClassifyOptions::default(),
            BreakdownMode::Absolute,
        );

        let at = |key: &str| {
            matrix
                .labels
                .iter()
                .position(|label| *label == key)
                .expect("every visible statistic has a row")
        };
        let top_pair = at("top pair");
        let flushdraw = at("flushdraw");

        // The diagonal is always one, and the row totals match the panel.
        for (row, cells) in matrix.rows.iter().enumerate() {
            if matrix.totals[row] > 0.0 {
                assert!((cells[row] - 1.0).abs() < 1e-9, "diagonal at {row}");
            }
            for value in cells {
                assert!((0.0..=1.0 + 1e-9).contains(value));
            }
        }
        assert!(
            (matrix.totals[top_pair] - panel.row(StatId::TOP_PAIR).unwrap().combos).abs() < 1e-9
        );

        // Bayes holds: P(fd|tp) * P(tp) == P(tp|fd) * P(fd).
        let left = matrix.rows[top_pair][flushdraw] * matrix.totals[top_pair];
        let right = matrix.rows[flushdraw][top_pair] * matrix.totals[flushdraw];
        assert!((left - right).abs() < 1e-9, "{left} vs {right}");
        assert!(matrix.rows[top_pair][flushdraw] > 0.0);
    }

    #[test]
    fn highlighting_is_a_share_of_what_the_cell_holds() {
        let (range, stats) = setup("AKs,AA", "Kc Qh Jh");
        let at = |hand: &str, mask| {
            highlight(&range, &stats, mask)
                [crate::cards::HandClass::parse(hand).unwrap().index() as usize]
        };
        // Every live AKs is top pair, so the cell lights all the way.
        assert_eq!(at("AKs", StatId::TOP_PAIR.mask()), 1.0);
        assert_eq!(at("AA", StatId::TOP_PAIR.mask()), 0.0);
        // One of the three live AKs combinations also has the heart flushdraw, so
        // the cell lights a third - visible, and honest about how much it is.
        let share = at("AKs", StatId::FLUSH_DRAW.mask());
        assert!(
            (share - 1.0 / 3.0).abs() < 1e-6,
            "flushdraw share came out at {share}"
        );
        // An empty mask lights whatever the range holds.
        assert_eq!(at("AA", StatMask::EMPTY), 1.0);
        assert_eq!(at("72o", StatMask::EMPTY), 0.0);
    }
}
