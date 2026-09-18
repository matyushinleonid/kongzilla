//! Preflop charts to start from.
//!
//! Flopzilla ships an empty predef tree: you build your own ranges or import
//! someone else's. That is fine once you have a library, and useless on day one.
//!
//! These are chip-EV tournament solutions at several stack depths, plus a raked
//! cash game, read out of GTO Wizard screenshots. Neither the screenshots nor
//! the reader that turns them into numbers is in the repository: what the build
//! eats is the generated table beside this file, and `make charts` rebuilds it
//! for whoever has the images.
//!
//! # The solver's own frequencies
//!
//! Solvers mix, and the charts keep the mix: a hand raised a third of the time
//! arrives at a third weight. The matrix draws a weight as a part-filled cell, so
//! a mixed strategy stays legible without being rounded into whole combos. The
//! screenshots only distinguish hands at the cell level, so every combo of a cell
//! carries the cell's weight.

use crate::cards::HandClass;
use crate::error::ParseError;
use crate::range::Range;

use crate::library_charts::CHARTS;

/// A stack depth the library covers.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Stack {
    /// Around 100 big blinds.
    Bb100,
    /// Around 80 big blinds.
    Bb80,
    /// Around 60 big blinds.
    Bb60,
    /// Around 40 big blinds.
    Bb40,
    /// Around 20 big blinds.
    Bb20,
    /// Six-handed NL25 cash at 100 big blinds - a raked game, so its ranges are
    /// tighter than the tournament ones at the same depth. Grouped here because
    /// this is where the reader picks between them, not because it is a depth.
    Nl25,
    /// The same six-handed cash game at 100 big blinds without the rake, which
    /// is the game a chip-EV solution answers. Wider than the raked ranges, and
    /// the small blind limps often enough that the big blind has an isolate.
    Cash100,
}

impl Stack {
    /// Every depth, deepest first.
    pub const ALL: [Stack; 7] = [
        Self::Bb100,
        Self::Bb80,
        Self::Bb60,
        Self::Bb40,
        Self::Bb20,
        Self::Nl25,
        Self::Cash100,
    ];

    /// A stable identifier, which is also the chip's label.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Bb100 => "100bb",
            Self::Bb80 => "80bb",
            Self::Bb60 => "60bb",
            Self::Bb40 => "40bb",
            Self::Bb20 => "20bb",
            Self::Nl25 => "NL25",
            Self::Cash100 => "cEV",
        }
    }

    /// Which game this belongs to. Cash and tournaments are not comparable, so
    /// they are not offered side by side as if they were two depths of one thing.
    pub const fn game(self) -> &'static str {
        match self {
            Self::Nl25 | Self::Cash100 => "cash",
            _ => "mtt",
        }
    }

    /// What the depth is, in words, for the chip's tooltip.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Nl25 => "Six-handed NL25 cash, 100bb effective, raked, cold calls at 2.5x",
            Self::Cash100 => "Six-handed cash, 100bb effective, no rake, cold calls at 2.5x",
            _ => "Eight-handed tournament play, chip-EV, no rake",
        }
    }
}

/// A seat at an eight-handed table.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
#[allow(missing_docs)]
pub enum Seat {
    Utg,
    Utg1,
    Lj,
    Hj,
    Co,
    Btn,
    Sb,
    Bb,
}

impl Seat {
    /// Every seat that opens the pot, earliest first.
    pub const OPENERS: [Seat; 7] = [
        Self::Utg,
        Self::Utg1,
        Self::Lj,
        Self::Hj,
        Self::Co,
        Self::Btn,
        Self::Sb,
    ];

    /// A stable identifier.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Utg => "utg",
            Self::Utg1 => "utg1",
            Self::Lj => "lj",
            Self::Hj => "hj",
            Self::Co => "co",
            Self::Btn => "btn",
            Self::Sb => "sb",
            Self::Bb => "bb",
        }
    }

    /// The label on the chip.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Utg => "UTG",
            Self::Utg1 => "UTG1",
            Self::Lj => "LJ",
            Self::Hj => "HJ",
            Self::Co => "CO",
            Self::Btn => "BTN",
            Self::Sb => "SB",
            Self::Bb => "BB",
        }
    }
}

/// What the chart is a strategy for.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash)]
pub enum Spot {
    /// Raise first in.
    Open,
    /// The small blind's whole raise-first-in strategy: limps and raises together,
    /// because both put it in the pot.
    RaiseFirstIn,
    /// Everything the big blind continues with: calls and three-bets together,
    /// because that is the range you face on the flop.
    Defend,
    /// The big blind raising over a small-blind limp.
    Isolate,
}

impl Spot {
    /// A stable identifier.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::RaiseFirstIn => "rfi",
            Self::Defend => "defend",
            Self::Isolate => "isolate",
        }
    }
}

/// One chart.
#[derive(Clone, Copy, Debug)]
pub struct Chart {
    /// Stable identifier, such as `mtt-100bb-open-btn`.
    pub id: &'static str,
    /// Which stack depth.
    pub stack: Stack,
    /// What the chart is a strategy for.
    pub spot: Spot,
    /// Whose strategy it is.
    pub seat: Seat,
    /// The seat being faced, for a defence or an isolate.
    pub versus: Option<Seat>,
    /// The raise the chart is about, in big blinds: the hero's own for an open or
    /// an isolate, the one being faced for a defence.
    pub size_bb: f32,
    /// Per-cell weight in per mille, so 333 is a third of the time.
    pub(crate) hands: &'static [(&'static str, u16)],
}

impl Chart {
    /// Builds the range, each cell at the weight the solver plays it.
    pub fn range(&self) -> Result<Range, ParseError> {
        let mut range = Range::empty();
        for (hand, per_mille) in self.hands {
            let class = HandClass::parse(hand)?;
            range.set_class(class, f32::from(*per_mille) / 1000.0);
        }
        Ok(range)
    }

    /// The weighted number of combos the chart holds.
    pub fn combos(&self) -> f64 {
        self.hands
            .iter()
            .map(|(hand, per_mille)| {
                let size = match hand.len() {
                    2 => 6.0,
                    _ if hand.ends_with('s') => 4.0,
                    _ => 12.0,
                };
                size * f64::from(*per_mille) / 1000.0
            })
            .sum()
    }

    /// The chart's share of all 1326 combos, as a percentage.
    pub fn percent(&self) -> f64 {
        self.combos() / crate::cards::NUM_COMBOS as f64 * 100.0
    }

    /// How many cells the solver plays only part of the time.
    pub fn mixed_cells(&self) -> usize {
        self.hands
            .iter()
            .filter(|(_, per_mille)| *per_mille < 1000)
            .count()
    }

    /// The name shown on the chip.
    pub fn label(&self) -> &'static str {
        match self.spot {
            Spot::Isolate => "SB limp",
            _ => self.versus.unwrap_or(self.seat).label(),
        }
    }

    /// One line saying what the chart is, including the size it is about.
    pub fn description(&self) -> String {
        let size = format_size(self.size_bb);
        match self.spot {
            Spot::Open => format!("{} opens to {size} bb", self.seat.label()),
            Spot::RaiseFirstIn => format!(
                "{} limps or raises to {size} bb, first in",
                self.seat.label()
            ),
            Spot::Defend => format!(
                "BB continues against a {} raise to {size} bb - calls and three-bets",
                self.versus.map_or("", Seat::label)
            ),
            Spot::Isolate => format!("BB raises to {size} bb over a small-blind limp"),
        }
    }
}

fn format_size(size: f32) -> String {
    let text = format!("{size:.2}");
    text.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Every chart.
pub fn charts() -> &'static [Chart] {
    CHARTS
}

/// Looks a chart up by its identifier.
pub fn chart_by_id(id: &str) -> Option<&'static Chart> {
    CHARTS.iter().find(|chart| chart.id == id)
}

/// Looks a chart up by stack depth, spot and the seat it concerns.
pub fn chart_for(stack: Stack, spot: Spot, seat: Seat) -> Option<&'static Chart> {
    CHARTS.iter().find(|chart| {
        chart.stack == stack && chart.spot == spot && chart.versus.unwrap_or(chart.seat) == seat
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chart(id: &str) -> &'static Chart {
        chart_by_id(id).unwrap_or_else(|| panic!("no chart {id}"))
    }

    #[test]
    fn every_chart_is_addressable_and_parses() {
        // Four complete tournament depths, twenty blinds without the limp to
        // isolate, and two six-handed cash games with no UTG1 or LJ to speak
        // of - the raked one without a limp to isolate either, the rakeless
        // one with.
        assert_eq!(CHARTS.len(), 4 * 15 + 14 + 10 + 11);
        for chart in CHARTS {
            let range = chart
                .range()
                .unwrap_or_else(|e| panic!("{}: {e}", chart.id));
            // Weights are f32 in the range and f64 here, so allow a thousandth
            // of a combo of drift across a hundred-odd cells.
            assert!(
                (range.combo_count() - chart.combos()).abs() < 1e-3,
                "{} lost weight building the range: {} vs {}",
                chart.id,
                range.combo_count(),
                chart.combos()
            );
            assert_eq!(chart_by_id(chart.id).map(|c| c.id), Some(chart.id));
            assert!(chart.description().contains("bb"));
            assert!(chart.size_bb > 1.0 && chart.size_bb < 20.0);
        }
        assert!(chart_by_id("nope").is_none());
    }

    #[test]
    fn the_solver_mix_survives() {
        // A chart that rounded every cell to all or nothing would have lost the
        // thing the solver actually said.
        let defend = chart("mtt-40bb-isolate-sb");
        assert!(
            defend.mixed_cells() > 20,
            "{} kept only {} mixed cells",
            defend.id,
            defend.mixed_cells()
        );
        let range = defend.range().unwrap();
        let weights: Vec<f32> = range.iter().map(|(_, weight)| weight).collect();
        assert!(
            weights.iter().any(|w| *w > 0.0 && *w < 1.0),
            "no partial weights"
        );
        assert!(
            weights.iter().any(|w| (*w - 1.0).abs() < 1e-6),
            "no full weights"
        );
    }

    #[test]
    fn opens_widen_with_position() {
        for stack in Stack::ALL {
            let mut previous = 0.0;
            // Six-handed cash has no UTG1 or LJ; the seats it does have still
            // have to come in order.
            for seat in [
                Seat::Utg,
                Seat::Utg1,
                Seat::Lj,
                Seat::Hj,
                Seat::Co,
                Seat::Btn,
            ] {
                let Some(chart) = chart_for(stack, Spot::Open, seat) else {
                    continue;
                };
                let percent = chart.percent();
                assert!(
                    percent > previous,
                    "{}: {} at {percent:.1}% is not wider than the seat before",
                    stack.key(),
                    seat.label()
                );
                previous = percent;
            }
        }
    }

    #[test]
    fn opens_tighten_as_stacks_shorten() {
        // A button with twenty blinds cannot play as many hands as one with a
        // hundred: there is no room left to outplay anyone after the flop.
        let at = |stack| chart_for(stack, Spot::Open, Seat::Btn).unwrap().percent();
        assert!(at(Stack::Bb100) > at(Stack::Bb40));
        assert!(at(Stack::Bb40) > at(Stack::Bb20));
    }

    #[test]
    fn defences_widen_against_later_openers_at_the_same_price() {
        // Position is not the only thing that moves a defence: the raise size does
        // too, so compare within a size rather than across.
        for stack in [Stack::Bb100, Stack::Bb80, Stack::Bb60, Stack::Bb40] {
            let at = |seat| chart_for(stack, Spot::Defend, seat).unwrap();
            for (early, late) in [
                (Seat::Utg, Seat::Utg1),
                (Seat::Lj, Seat::Hj),
                (Seat::Hj, Seat::Co),
                (Seat::Co, Seat::Btn),
            ] {
                let (a, b) = (at(early), at(late));
                if (a.size_bb - b.size_bb).abs() < 0.01 {
                    assert!(
                        a.percent() < b.percent(),
                        "{}: vs {} {:.1}% should be tighter than vs {} {:.1}%",
                        stack.key(),
                        early.label(),
                        a.percent(),
                        late.label(),
                        b.percent()
                    );
                }
            }
            for seat in [Seat::Utg, Seat::Lj, Seat::Co, Seat::Btn] {
                assert!(at(seat).percent() > chart_for(stack, Spot::Open, seat).unwrap().percent());
            }
        }
    }

    #[test]
    fn a_smaller_raise_is_defended_wider() {
        // At eighty blinds UTG1 opens to 2bb and the lojack to 2.1bb; the cheaper
        // raise gets the wider defence even though it comes from an earlier seat.
        let early = chart("mtt-80bb-defend-utg1");
        let late = chart("mtt-80bb-defend-lj");
        assert!(early.size_bb < late.size_bb);
        assert!(early.percent() > late.percent());
    }

    #[test]
    fn twenty_blinds_has_everything_but_the_limp_to_isolate() {
        for seat in [Seat::Utg, Seat::Btn] {
            assert!(chart_for(Stack::Bb20, Spot::Open, seat).is_some());
            assert!(chart_for(Stack::Bb20, Spot::Defend, seat).is_some());
        }
        assert!(chart_for(Stack::Bb20, Spot::RaiseFirstIn, Seat::Sb).is_some());
        // That shallow the small blind raises or folds, so there is no limp.
        assert!(chart_for(Stack::Bb20, Spot::Isolate, Seat::Sb).is_none());
    }

    #[test]
    fn the_cash_game_is_a_six_handed_ring_and_tighter_for_the_rake() {
        // Six-handed: the two seats an eight-handed table adds are not there.
        assert!(chart_for(Stack::Nl25, Spot::Open, Seat::Utg1).is_none());
        assert!(chart_for(Stack::Nl25, Spot::Open, Seat::Lj).is_none());
        for seat in [Seat::Utg, Seat::Hj, Seat::Co, Seat::Btn] {
            assert!(
                chart_for(Stack::Nl25, Spot::Open, seat).is_some(),
                "{seat:?}"
            );
            assert!(
                chart_for(Stack::Nl25, Spot::Defend, seat).is_some(),
                "{seat:?}"
            );
        }
        assert!(chart_for(Stack::Nl25, Spot::RaiseFirstIn, Seat::Sb).is_some());

        // Rake is paid out of every pot won, so marginal hands stop being worth
        // playing: the same seat opens tighter here than at the same depth in a
        // tournament, where there is none.
        let cash = chart_for(Stack::Nl25, Spot::Open, Seat::Btn).unwrap();
        let mtt = chart_for(Stack::Bb100, Spot::Open, Seat::Btn).unwrap();
        assert!(
            cash.percent() < mtt.percent(),
            "raked BTN opens {:.1}%, chip-EV opens {:.1}%",
            cash.percent(),
            mtt.percent()
        );

        // The raise itself is 2.5 either way at this depth.
        assert_eq!(cash.size_bb, 2.5);
    }

    #[test]
    fn the_numbers_match_the_solutions_they_came_from() {
        // Reading the fill of every cell should reproduce what the tool reports
        // beside the matrix, to within the width of a pixel boundary.
        let expect = |id: &str, percent: f64| {
            let got = chart(id).percent();
            assert!(
                (got - percent).abs() < 0.3,
                "{id} came out at {got:.2}%, expected {percent}%"
            );
        };
        expect("mtt-100bb-open-utg", 16.8);
        expect("mtt-100bb-open-btn", 54.3);
        expect("mtt-80bb-open-btn", 54.6);
        expect("mtt-20bb-open-utg", 17.2);
        expect("mtt-100bb-defend-btn", 77.3);
    }

    #[test]
    fn sizes_and_labels_read_sensibly() {
        assert_eq!(chart("mtt-100bb-open-utg").size_bb, 2.1);
        assert_eq!(chart("mtt-40bb-open-btn").size_bb, 2.1);
        assert_eq!(chart("mtt-100bb-open-btn").label(), "BTN");
        assert_eq!(chart("mtt-100bb-isolate-sb").label(), "SB limp");
        assert!(chart("mtt-100bb-defend-btn")
            .description()
            .contains("BTN raise to 2.5 bb"));
        assert!(chart("mtt-40bb-isolate-sb")
            .description()
            .contains("3.25 bb"));
    }
}
