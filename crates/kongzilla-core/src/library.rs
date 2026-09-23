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
    /// Around 30 big blinds. The small blind's own open is not solved here:
    /// that one screenshot is missing from the shoot, and a chart nobody has
    /// is better absent than guessed at.
    Bb30,
    /// Around 20 big blinds.
    Bb20,
    /// Six-handed NL10 cash at 100 big blinds, raked. The same game a stake
    /// down: more rake per pot in proportion, so tighter again than NL25.
    Nl10,
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
    pub const ALL: [Stack; 9] = [
        Self::Bb100,
        Self::Bb80,
        Self::Bb60,
        Self::Bb40,
        Self::Bb30,
        Self::Bb20,
        Self::Nl10,
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
            Self::Bb30 => "30bb",
            Self::Bb20 => "20bb",
            Self::Nl10 => "NL10",
            Self::Nl25 => "NL25",
            Self::Cash100 => "cEV",
        }
    }

    /// Which game this belongs to. Cash and tournaments are not comparable, so
    /// they are not offered side by side as if they were two depths of one thing.
    pub const fn game(self) -> &'static str {
        match self {
            Self::Nl10 | Self::Nl25 | Self::Cash100 => "cash",
            _ => "mtt",
        }
    }

    /// What the depth is, in words, for the chip's tooltip.
    pub const fn description(self) -> &'static str {
        match self {
            Self::Nl10 => "Six-handed NL10 cash, 100bb effective, raked, cold calls at 2.5x",
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
    /// Every seat, earliest first. The order is the order of action, so a seat
    /// behind another is simply greater than it.
    pub const ALL: [Seat; 8] = [
        Self::Utg,
        Self::Utg1,
        Self::Lj,
        Self::Hj,
        Self::Co,
        Self::Btn,
        Self::Sb,
        Self::Bb,
    ];

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
    /// What a seat does against somebody else's open.
    ///
    /// Calling and three-betting are separate questions with separate answers,
    /// and the chart carries both - see [`Chart::offers`]. Merged, they are the
    /// range that arrives on the flop; apart, they are the cold call and the
    /// three-bet.
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

/// One way of putting a hand in the pot.
///
/// Folding is not one: a chart is what a seat does with the hands it keeps.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Action {
    /// Putting in what is already there - a limp, or a call of a raise.
    Call,
    /// Putting in more.
    Raise,
    /// Putting in the lot.
    Allin,
}

impl Action {
    /// Every action, in the order the charts store them.
    pub const ALL: [Action; 3] = [Self::Call, Self::Raise, Self::Allin];

    /// How many there are, which is how wide a chart's weights are.
    pub const COUNT: usize = 3;

    /// A stable identifier.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Call => "call",
            Self::Raise => "raise",
            Self::Allin => "allin",
        }
    }

    /// Reads a [`Action::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }

    /// This action and no other.
    pub const fn only(self) -> Actions {
        Actions([
            matches!(self, Self::Call),
            matches!(self, Self::Raise),
            matches!(self, Self::Allin),
        ])
    }
}

/// Which actions a range is being built from.
///
/// A chart holds what the solver does with every hand, split by what it does;
/// this says which of those to count. All of them is the range that arrives on
/// the flop. One of them is a question about that one decision - what does this
/// seat three-bet, what does it shove, what does it limp.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Actions([bool; Action::COUNT]);

impl Actions {
    /// Everything the solver does with the hands it keeps.
    pub const ALL: Self = Self([true; Action::COUNT]);

    /// None of them, which builds an empty range.
    pub const NONE: Self = Self([false; Action::COUNT]);

    /// Whether one action is counted.
    pub const fn has(self, action: Action) -> bool {
        self.0[action as usize]
    }

    /// The same set with one action counted, or not.
    pub const fn with(mut self, action: Action, on: bool) -> Self {
        self.0[action as usize] = on;
        self
    }

    /// Reads a comma-separated list of [`Action::key`]s.
    pub fn parse(keys: &str) -> Self {
        let mut actions = Self::NONE;
        for key in keys.split(',') {
            if let Some(action) = Action::from_key(key.trim()) {
                actions = actions.with(action, true);
            }
        }
        actions
    }

    /// What one cell's weights come to, counting only these actions.
    ///
    /// Clamped at one: the actions of a cell are shares of it and cannot come
    /// to more than the whole, but they are rounded to a thousandth each and
    /// three roundings can carry it over.
    fn weight_of(self, per_mille: [u16; Action::COUNT]) -> f32 {
        let total: u32 = Action::ALL
            .into_iter()
            .filter(|action| self.has(*action))
            .map(|action| u32::from(per_mille[action as usize]))
            .sum();
        (total as f32 / 1000.0).min(1.0)
    }
}

impl Default for Actions {
    fn default() -> Self {
        Self::ALL
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
    /// Per-cell weight in per mille, action by action, in the order of
    /// [`Action::ALL`] - so `[250, 0, 0]` is a cell called a quarter of the
    /// time and nothing else.
    ///
    /// Apart rather than added up, because they are separate decisions: a seat
    /// facing an open calls some hands and raises others, and which of those a
    /// reader wants on the table is theirs to say.
    pub(crate) hands: &'static [(&'static str, [u16; Action::COUNT])],
    /// The hands the chart plays at no gain: their EV is zero.
    ///
    /// A solver's range has a fringe it is indifferent about: hands it calls a
    /// fifth of the time and would break even folding. They are in the chart
    /// because they are in the solution, and they are named here because a
    /// reader studying the spot may want the part that actually makes money -
    /// the hands you would be wrong to fold - without the fringe that only
    /// balances it.
    pub(crate) zero_ev: &'static [&'static str],
}

impl Chart {
    /// Builds the range, each cell at the weight the solver plays it.
    pub fn range(&self) -> Result<Range, ParseError> {
        self.build(Actions::ALL, false)
    }

    /// The range built from some of its actions only.
    pub fn range_of(&self, actions: Actions) -> Result<Range, ParseError> {
        self.build(actions, false)
    }

    /// The same, without the hands whose EV is zero.
    pub fn range_of_without_zero_ev(&self, actions: Actions) -> Result<Range, ParseError> {
        self.build(actions, true)
    }

    /// The same range without the hands whose EV is zero.
    ///
    /// What is left is the part of the solution that wins something. It is not
    /// the solution - folding the fringe is exploitable, which is why the
    /// solver does not - but it is the part worth learning first, and the
    /// difference between the two is worth seeing.
    pub fn range_without_zero_ev(&self) -> Result<Range, ParseError> {
        self.build(Actions::ALL, true)
    }

    /// Whether any of what the chart plays is played at no gain.
    pub fn has_zero_ev(&self) -> bool {
        !self.zero_ev.is_empty()
    }

    fn build(&self, actions: Actions, drop_zero_ev: bool) -> Result<Range, ParseError> {
        let mut range = Range::empty();
        for (hand, per_mille) in self.hands {
            if drop_zero_ev && self.zero_ev.contains(hand) {
                continue;
            }
            let weight = actions.weight_of(*per_mille);
            if weight <= 0.0 {
                continue;
            }
            let class = HandClass::parse(hand)?;
            range.set_class(class, weight);
        }
        Ok(range)
    }

    /// The weighted number of combos the chart holds.
    pub fn combos(&self) -> f64 {
        self.combos_of(Actions::ALL)
    }

    /// The same, counting only some of its actions.
    pub fn combos_of(&self, actions: Actions) -> f64 {
        self.hands
            .iter()
            .map(|(hand, per_mille)| {
                let size = match hand.len() {
                    2 => 6.0,
                    _ if hand.ends_with('s') => 4.0,
                    _ => 12.0,
                };
                size * f64::from(actions.weight_of(*per_mille))
            })
            .sum()
    }

    /// The chart's share of all 1326 combos, as a percentage.
    pub fn percent(&self) -> f64 {
        self.percent_of(Actions::ALL)
    }

    /// The same, counting only some of its actions.
    pub fn percent_of(&self, actions: Actions) -> f64 {
        self.combos_of(actions) / crate::cards::NUM_COMBOS as f64 * 100.0
    }

    /// Which actions this chart actually uses, and what each is called here.
    ///
    /// A chart only offers what the solver does in that spot: an opening range
    /// has raises, and at twenty blinds it has shoves as well; nobody limps
    /// facing a raise. The panel puts a switch against each of these and none
    /// against the rest, because a switch for a thing the chart never does is
    /// a switch that does nothing.
    pub fn offers(&self) -> Vec<(Action, &'static str, f64)> {
        Action::ALL
            .into_iter()
            .filter(|action| self.hands.iter().any(|(_, per)| per[*action as usize] > 0))
            .map(|action| {
                (
                    action,
                    self.action_label(action),
                    self.percent_of(action.only()),
                )
            })
            .collect()
    }

    /// What one action is called in this spot.
    ///
    /// The same action goes by different names depending on what it answers: a
    /// call is a limp when nobody has raised and a cold call when somebody has,
    /// and a raise is an open, an isolate or a three-bet for the same reason.
    fn action_label(&self, action: Action) -> &'static str {
        match (self.spot, action) {
            (_, Action::Allin) => "pushes",
            (Spot::RaiseFirstIn, Action::Call) => "limps",
            (Spot::Defend, Action::Call) => "cold calls",
            (_, Action::Call) => "calls",
            (Spot::Open | Spot::RaiseFirstIn, Action::Raise) => "opens",
            (Spot::Defend, Action::Raise) => "3-bets",
            (Spot::Isolate, Action::Raise) => "raises",
        }
    }

    /// How many cells the solver plays only part of the time.
    pub fn mixed_cells(&self) -> usize {
        self.hands
            .iter()
            .filter(|(_, per_mille)| per_mille.iter().sum::<u16>() < 1000)
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
                "{} against a {} raise to {size} bb",
                self.seat.label(),
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
        // Six complete tournament depths, and three six-handed cash games with
        // no UTG1 or LJ to speak of - each with the five opens, the five
        // defences and the limp to isolate.
        //
        // And on top of those, what each other seat does facing an open: at
        // every tournament depth, twenty-eight spots less the seven the big
        // blind already had a defence for; and in every cash game, fifteen
        // less the five it had.
        assert_eq!(CHARTS.len(), 6 * 15 + 3 * 11 + 6 * (28 - 7) + 3 * (15 - 5));
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
    fn dropping_the_zero_ev_hands_leaves_the_part_that_wins() {
        // Defending the big blind is where the fringe is widest: the last hands
        // in are called precisely because folding them would be no worse.
        let defence = chart("mtt-20bb-defend-sb");
        assert!(defence.has_zero_ev());
        let whole = defence.range().expect("it parses");
        let winning = defence
            .range_without_zero_ev()
            .expect("so does the trimmed one");
        assert!(winning.combo_count() < whole.combo_count());
        assert!(winning.combo_count() > 0.0);

        // Every hand it drops is one the chart held, and nothing else moves:
        // the hands that stay keep the weight the solver gave them.
        for (hand, per_mille) in defence.hands {
            let class = HandClass::parse(hand).expect("a hand the chart names");
            let kept = winning.class_weight(class);
            if defence.zero_ev.contains(hand) {
                assert_eq!(kept, 0.0, "{hand} is a 0-EV hand and should be gone");
            } else {
                let whole = f32::from(per_mille.iter().sum::<u16>()) / 1000.0;
                assert!(
                    (kept - whole.min(1.0)).abs() < 1e-6,
                    "{hand} changed weight"
                );
            }
        }

        // A chart with nothing to drop gives back the same range either way,
        // which is what stops the option looking broken where it does nothing.
        let opener = chart("mtt-100bb-open-utg");
        if !opener.has_zero_ev() {
            assert_eq!(
                opener.range().unwrap().combo_count(),
                opener.range_without_zero_ev().unwrap().combo_count()
            );
        }
    }

    #[test]
    fn the_zero_ev_fringe_is_the_bottom_of_a_range_not_the_top() {
        // Every hand named as 0-EV is a hand the chart actually plays -
        // otherwise the option would claim to drop something that was never
        // there. Note that being played *always* does not save a hand: the
        // last hand a solver opens is opened every time and still gains
        // nothing measurable, which is exactly the fringe this is about.
        for chart in CHARTS {
            for hand in chart.zero_ev {
                assert!(
                    chart.hands.iter().any(|(name, _)| name == hand),
                    "{}: {hand} is not in the chart",
                    chart.id
                );
            }
            // And it is the bottom of the range, never the top: no solution is
            // indifferent about aces.
            for premium in ["AA", "KK", "AKs"] {
                assert!(
                    !chart.zero_ev.contains(&premium),
                    "{}: {premium} cannot be 0-EV",
                    chart.id
                );
            }
        }
    }

    #[test]
    fn twenty_blinds_is_a_complete_depth_like_the_others() {
        for seat in [Seat::Utg, Seat::Btn] {
            assert!(chart_for(Stack::Bb20, Spot::Open, seat).is_some());
            assert!(chart_for(Stack::Bb20, Spot::Defend, seat).is_some());
        }
        assert!(chart_for(Stack::Bb20, Spot::RaiseFirstIn, Seat::Sb).is_some());

        // The small blind limps this shallow too, so the big blind has an
        // isolate - and that shallow it is a shove or a raise rather than a
        // call: there is no folding a limp when the blind is already in.
        let iso = chart_for(Stack::Bb20, Spot::Isolate, Seat::Sb).expect("the limp is solved");
        let range = iso.range().expect("it parses");
        assert!(range.combo_count() > 0.0);
        assert!(iso.size_bb <= 3.0, "raising a limp is small this shallow");
    }

    #[test]
    fn every_tournament_depth_holds_the_same_spots() {
        // Thirty blinds arrived last and in its own order - the three-bets
        // first, the small blind's own open an hour after everything else - so
        // this says that what came out of that shoot is the same set of spots
        // as the depths shot before it, rather than whatever it happened to
        // cover.
        for stack in Stack::ALL.into_iter().filter(|stack| stack.game() == "mtt") {
            assert!(
                chart_for(stack, Spot::RaiseFirstIn, Seat::Sb).is_some(),
                "{stack:?}"
            );
            assert!(
                chart_for(stack, Spot::Isolate, Seat::Sb).is_some(),
                "{stack:?}"
            );
            for seat in [
                Seat::Utg,
                Seat::Utg1,
                Seat::Lj,
                Seat::Hj,
                Seat::Co,
                Seat::Btn,
            ] {
                assert!(
                    chart_for(stack, Spot::Open, seat).is_some(),
                    "{stack:?} {seat:?}"
                );
                assert!(
                    chart_for(stack, Spot::Defend, seat).is_some(),
                    "{stack:?} {seat:?}"
                );
            }
            assert!(
                chart_for(stack, Spot::Defend, Seat::Sb).is_some(),
                "{stack:?}"
            );
        }
    }

    #[test]
    fn every_seat_behind_an_opener_has_an_answer_to_it() {
        // Twenty-eight per depth: seven openers, each answered by everybody
        // still to act. The big blind's are the defences it was always shown,
        // since a defence is exactly that - what a seat does against an open.
        for stack in Stack::ALL.into_iter().filter(|stack| stack.game() == "mtt") {
            let mut seen = 0;
            for opener in Seat::OPENERS {
                let behind: Vec<Seat> = Seat::ALL
                    .into_iter()
                    .filter(|seat| *seat > opener)
                    .collect();
                assert!(!behind.is_empty());
                for seat in behind {
                    let chart = chart_at(stack, Spot::Defend, seat, opener);
                    assert!(chart.percent() > 0.0, "{stack:?} {seat:?} vs {opener:?}");
                    seen += 1;
                }
            }
            assert_eq!(seen, 28, "{stack:?}");
        }
    }

    #[test]
    fn a_cash_game_answers_an_open_from_four_seats() {
        // Six-handed, so the seats behind an opener are fewer - and the two
        // an eight-handed table adds are not there to ask about.
        for stack in Stack::ALL
            .into_iter()
            .filter(|stack| stack.game() == "cash")
        {
            let mut seen = 0;
            for opener in [Seat::Utg, Seat::Hj, Seat::Co, Seat::Btn, Seat::Sb] {
                for seat in [Seat::Hj, Seat::Co, Seat::Btn, Seat::Sb, Seat::Bb] {
                    if seat <= opener {
                        continue;
                    }
                    let chart = chart_at(stack, Spot::Defend, seat, opener);
                    assert!(chart.percent() > 0.0, "{stack:?} {seat:?} vs {opener:?}");
                    seen += 1;
                }
            }
            assert_eq!(seen, 15, "{stack:?}");
            for absent in [Seat::Utg1, Seat::Lj] {
                assert!(
                    !CHARTS.iter().any(|chart| chart.stack == stack
                        && chart.spot == Spot::Defend
                        && chart.seat == absent),
                    "{stack:?} has no {absent:?} to answer with"
                );
            }
        }
    }

    #[test]
    fn cold_calling_thins_out_as_the_stacks_shorten() {
        // A cold call is a hand taken to the flop to play, and there is less
        // left to play it with every time the stacks come down - so a seat
        // that flats to use its position flats less of what it continues with.
        for (seat, opener) in [
            (Seat::Utg1, Seat::Utg),
            (Seat::Btn, Seat::Utg),
            (Seat::Sb, Seat::Btn),
        ] {
            let calls =
                |stack| chart_at(stack, Spot::Defend, seat, opener).percent_of(Action::Call.only());
            assert!(
                calls(Stack::Bb40) < calls(Stack::Bb100),
                "{seat:?} vs {opener:?}: calls {:.1}% at forty, {:.1}% at a hundred",
                calls(Stack::Bb40),
                calls(Stack::Bb100)
            );
        }

        // The big blind is the other way round. It is priced in whatever it
        // holds, and with less room to play after the flop it takes the price
        // more often rather than raising - the one seat where a shorter stack
        // means more calling, not less.
        let blind = |stack| {
            chart_at(stack, Spot::Defend, Seat::Bb, Seat::Btn).percent_of(Action::Call.only())
        };
        assert!(
            blind(Stack::Bb40) > blind(Stack::Bb100),
            "BB vs BTN: calls {:.1}% at forty, {:.1}% at a hundred",
            blind(Stack::Bb40),
            blind(Stack::Bb100)
        );

        // Three-bets do not move one way with the stack, which is why nothing
        // here claims they do: they widen down to sixty blinds and tighten
        // again at forty, where going back in commits most of what is behind.
    }

    #[test]
    fn a_chart_offers_what_the_solver_actually_does() {
        // Facing an open a seat calls some hands and raises others, and the
        // two are separate answers rather than one merged one.
        let chart = chart_for_versus(Spot::Defend, Seat::Btn, Seat::Co);
        let offers = chart.offers();
        assert_eq!(
            offers.iter().map(|(_, what, _)| *what).collect::<Vec<_>>(),
            ["cold calls", "3-bets"],
            "no shoving at a hundred blinds"
        );

        // The parts come to the whole. They add rather than merge: a cell the
        // solver calls a third of the time and raises the rest is one cell in
        // both of them, and the third and the two thirds are the whole of it -
        // which is why this counts combos rather than taking a union.
        let whole = chart.range().unwrap();
        let calls = chart.range_of(Action::Call.only()).unwrap();
        let raises = chart.range_of(Action::Raise.only()).unwrap();
        assert!(raises.combo_count() > 0.0 && calls.combo_count() > 0.0);
        assert!(
            (calls.combo_count() + raises.combo_count() - whole.combo_count()).abs() < 0.01,
            "{} and {} against {}",
            calls.combo_count(),
            raises.combo_count(),
            whole.combo_count()
        );
        // And neither reaches a hand the chart does not hold.
        let parts = calls.union(&raises);
        assert_eq!(parts.intersection(&whole), parts);
        // Aces always go back in, so there are none left to call with.
        assert_eq!(raises.class_weight(HandClass::parse("AA").unwrap()), 1.0);
        assert_eq!(calls.class_weight(HandClass::parse("AA").unwrap()), 0.0);

        // An open has no calls in it, and nobody shoves a hundred blinds.
        let open = chart_for(Stack::Bb100, Spot::Open, Seat::Btn).expect("an open");
        assert_eq!(
            open.offers()
                .iter()
                .map(|(_, what, _)| *what)
                .collect::<Vec<_>>(),
            ["opens"]
        );

        // Twenty blinds is where the shoving starts, and the small blind still
        // limps some of what it plays.
        let short = chart_for(Stack::Bb20, Spot::RaiseFirstIn, Seat::Sb).expect("a small blind");
        assert_eq!(
            short
                .offers()
                .iter()
                .map(|(_, what, _)| *what)
                .collect::<Vec<_>>(),
            ["limps", "opens", "pushes"]
        );
        for (_, what, percent) in short.offers() {
            assert!(percent > 0.0, "{what} is offered but never done");
        }
    }

    #[test]
    fn the_parts_of_a_chart_add_up_to_it() {
        for chart in CHARTS {
            let offers = chart.offers();
            assert!(!offers.is_empty(), "{} does nothing at all", chart.id);
            let parts: f64 = offers.iter().map(|(_, _, percent)| percent).sum();
            assert!(
                (parts - chart.percent()).abs() < 0.05,
                "{}: parts come to {parts:.2}%, the whole is {:.2}%",
                chart.id,
                chart.percent()
            );
            // And asking for none of them is asking for nothing.
            assert!(chart.range_of(Actions::NONE).unwrap().is_empty());
        }
    }

    /// One chart of a spot between two named seats, at a hundred blinds.
    fn chart_for_versus(spot: Spot, seat: Seat, versus: Seat) -> &'static Chart {
        chart_at(Stack::Bb100, spot, seat, versus)
    }

    /// The same, at whichever depth.
    fn chart_at(stack: Stack, spot: Spot, seat: Seat, versus: Seat) -> &'static Chart {
        CHARTS
            .iter()
            .find(|chart| {
                chart.stack == stack
                    && chart.spot == spot
                    && chart.seat == seat
                    && chart.versus == Some(versus)
            })
            .unwrap_or_else(|| panic!("no {spot:?} for {seat:?} vs {versus:?} at {stack:?}"))
    }

    #[test]
    fn the_stakes_are_three_readings_of_one_game() {
        // Each cash game holds the same spots, so a reader can put the same
        // question to all three and read the rake off the difference.
        for stack in [Stack::Nl10, Stack::Nl25, Stack::Cash100] {
            assert_eq!(stack.game(), "cash");
            for seat in [Seat::Utg, Seat::Hj, Seat::Co, Seat::Btn] {
                assert!(
                    chart_for(stack, Spot::Open, seat).is_some(),
                    "{stack:?} {seat:?}"
                );
                assert!(
                    chart_for(stack, Spot::Defend, seat).is_some(),
                    "{stack:?} {seat:?}"
                );
            }
            assert!(
                chart_for(stack, Spot::RaiseFirstIn, Seat::Sb).is_some(),
                "{stack:?}"
            );
            assert!(
                chart_for(stack, Spot::Defend, Seat::Sb).is_some(),
                "{stack:?}"
            );
            assert!(
                chart_for(stack, Spot::Isolate, Seat::Sb).is_some(),
                "{stack:?}"
            );
            // Six-handed: the two seats an eight-handed table adds are not there.
            assert!(
                chart_for(stack, Spot::Open, Seat::Utg1).is_none(),
                "{stack:?}"
            );
            assert!(
                chart_for(stack, Spot::Open, Seat::Lj).is_none(),
                "{stack:?}"
            );
        }

        // Rake tightens an opening range, and there is more of it per pot at a
        // lower stake - so the order of the three is the order of the rake.
        let opens = |stack| {
            chart_for(stack, Spot::Open, Seat::Utg)
                .expect("an opening chart")
                .percent()
        };
        assert!(opens(Stack::Nl10) < opens(Stack::Cash100), "rake tightens");
        assert!(opens(Stack::Nl25) < opens(Stack::Cash100), "rake tightens");
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
