//! A weighted set of starting hands.
//!
//! A range is a weight in `0.0..=1.0` for each of the 1326 combos. Weights are
//! carried everywhere rather than a plain in/out flag, because modern preflop
//! charts are mixed and a range that cannot express "three-bet a third of the
//! time" cannot import them.

use crate::cards::{CardSet, Combo, HandClass, NUM_CLASSES, NUM_COMBOS, RANK_ACE};
use crate::error::ParseError;
use crate::notation;
use crate::ranking::Ranking;

/// Weights for every combo in the deck.
#[derive(Clone, PartialEq)]
pub struct Range {
    weights: Box<[f32; NUM_COMBOS]>,
}

impl Range {
    /// A range holding nothing.
    pub fn empty() -> Self {
        Self {
            weights: Box::new([0.0; NUM_COMBOS]),
        }
    }

    /// Every combo at full weight.
    pub fn full() -> Self {
        Self {
            weights: Box::new([1.0; NUM_COMBOS]),
        }
    }

    /// Reads a range from text such as `AKs+, 77-99, QJo`.
    pub fn parse(text: &str) -> Result<Self, ParseError> {
        notation::parse(text)
    }

    /// Renders the range back to text.
    pub fn to_notation(&self) -> String {
        notation::format(self)
    }

    /// The same, written for a link: identical but for the weights, which go in
    /// packed. Both forms parse, so it is the same language either way.
    pub fn to_packed_notation(&self) -> String {
        notation::format_packed(self)
    }

    /// The weight of one combo.
    pub fn get(&self, combo: Combo) -> f32 {
        self.weights[combo.index() as usize]
    }

    /// Sets the weight of one combo, clamped to `0.0..=1.0`.
    pub fn set(&mut self, combo: Combo, weight: f32) {
        self.weights[combo.index() as usize] = weight.clamp(0.0, 1.0);
    }

    /// Sets every combo in a matrix cell to the same weight.
    pub fn set_class(&mut self, class: HandClass, weight: f32) {
        for combo in class.combos() {
            self.set(combo, weight);
        }
    }

    /// The mean weight across a matrix cell, which is what the cell renders as.
    pub fn class_weight(&self, class: HandClass) -> f32 {
        let mut total = 0.0;
        let mut count = 0.0;
        for combo in class.combos() {
            total += self.get(combo);
            count += 1.0;
        }
        if count == 0.0 {
            0.0
        } else {
            total / count
        }
    }

    /// Weighted combos per matrix cell, which is the small number each cell shows.
    pub fn class_combo_counts(&self) -> [f32; NUM_CLASSES] {
        let mut sums = [0.0f32; NUM_CLASSES];
        for combo in Combo::all() {
            sums[combo.class().index() as usize] += self.get(combo);
        }
        sums
    }

    /// Mean weight per matrix cell, for drawing the whole matrix in one pass.
    pub fn class_weights(&self) -> [f32; NUM_CLASSES] {
        let mut sums = [0.0f32; NUM_CLASSES];
        for combo in Combo::all() {
            sums[combo.class().index() as usize] += self.get(combo);
        }
        for class in HandClass::all() {
            sums[class.index() as usize] /= f32::from(class.combo_count());
        }
        sums
    }

    /// The weighted number of combos in the range.
    pub fn combo_count(&self) -> f64 {
        self.weights.iter().map(|w| f64::from(*w)).sum()
    }

    /// The weighted number of combos left after removing anything that clashes
    /// with `dead`.
    pub fn combo_count_excluding(&self, dead: CardSet) -> f64 {
        Combo::all()
            .filter(|c| !c.mask().intersects(dead))
            .map(|c| f64::from(self.get(c)))
            .sum()
    }

    /// The range's size as a share of all 1326 combos.
    pub fn percent_of_deck(&self) -> f64 {
        self.combo_count() / NUM_COMBOS as f64
    }

    /// Whether the range holds nothing.
    pub fn is_empty(&self) -> bool {
        self.weights.iter().all(|w| *w <= 0.0)
    }

    /// Every combo with a non-zero weight.
    pub fn iter(&self) -> impl Iterator<Item = (Combo, f32)> + '_ {
        Combo::all()
            .map(move |combo| (combo, self.get(combo)))
            .filter(|(_, weight)| *weight > 0.0)
    }

    /// Every combo with a non-zero weight that does not clash with `dead`.
    pub fn live(&self, dead: CardSet) -> Vec<(Combo, f32)> {
        self.iter()
            .filter(|(combo, _)| !combo.mask().intersects(dead))
            .collect()
    }

    /// The range with every hand that needs a blocked card taken out of it.
    ///
    /// A hand somebody else has been dealt is a hand this range cannot be
    /// holding, so it is not in the range any more - not merely drawn greyer.
    pub fn without_cards(&self, blocked: CardSet) -> Self {
        let mut out = self.clone();
        for (combo, weight) in self.iter() {
            if weight > 0.0 && combo.mask().intersects(blocked) {
                out.set(combo, 0.0);
            }
        }
        out
    }

    /// Adds `other` into this range, keeping the larger weight per combo.
    pub fn union(&self, other: &Self) -> Self {
        self.zip_with(other, f32::max)
    }

    /// Keeps only what both ranges hold, at the smaller weight.
    pub fn intersection(&self, other: &Self) -> Self {
        self.zip_with(other, f32::min)
    }

    /// Removes `other` from this range.
    pub fn difference(&self, other: &Self) -> Self {
        self.zip_with(other, |a, b| (a - b).max(0.0))
    }

    /// Multiplies every weight by `factor`.
    pub fn scaled(&self, factor: f32) -> Self {
        let mut out = self.clone();
        for weight in out.weights.iter_mut() {
            *weight = (*weight * factor).clamp(0.0, 1.0);
        }
        out
    }

    /// Everything the range does not hold, at full weight.
    pub fn inverted(&self) -> Self {
        let mut out = Self::empty();
        for combo in Combo::all() {
            out.set(combo, 1.0 - self.get(combo));
        }
        out
    }

    fn zip_with(&self, other: &Self, f: impl Fn(f32, f32) -> f32) -> Self {
        let mut out = Self::empty();
        for i in 0..NUM_COMBOS {
            out.weights[i] = f(self.weights[i], other.weights[i]).clamp(0.0, 1.0);
        }
        out
    }

    /// Every hand a quick button selects.
    pub fn preset(preset: Preset) -> Self {
        let mut range = Self::empty();
        for class in HandClass::all() {
            if preset.holds(class) {
                range.set_class(class, 1.0);
            }
        }
        range
    }

    /// The strongest `percent` of all starting hands under `ranking`.
    ///
    /// Whole matrix cells are added at a time, the way Flopzilla's slider behaves,
    /// so the realised percentage lands on the nearest cell boundary at or below
    /// the request.
    pub fn top_percent(percent: f64, ranking: Ranking) -> Self {
        let mut range = Self::empty();
        if percent <= 0.0 {
            return range;
        }
        let budget = (percent / 100.0) * NUM_COMBOS as f64;
        let mut used = 0.0;
        for class in ranking.order() {
            let size = f64::from(class.combo_count());
            if used + size > budget + 1e-9 {
                break;
            }
            range.set_class(*class, 1.0);
            used += size;
        }
        range
    }

    /// The band of the ranking between `from` and `to` percent.
    ///
    /// This is what the slider under the matrix selects: the red handle sets where
    /// the band starts and the blue handle where it ends, so `window(5, 20)` is a
    /// calling range that has already three-bet its best five percent. The handles
    /// are sorted, so neither can overtake the other.
    pub fn window(from: f64, to: f64, ranking: Ranking) -> Self {
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        Self::top_percent(high, ranking).without_top_percent(low, ranking)
    }

    /// What share of the deck sits in the matrix cells this range touches.
    ///
    /// Not [`Range::percent_of_deck`], which weighs each hand by how much of it
    /// is in: a chart that raises AJo half the time still touches all twelve of
    /// them. This is the figure the slider parks on, because the slider moves
    /// whole cells.
    pub fn cell_percent(&self) -> f64 {
        let held = self.class_combo_counts();
        let mut cells = 0.0;
        for class in HandClass::all() {
            if held[class.index() as usize] > 0.0 {
                cells += f64::from(class.combo_count());
            }
        }
        cells / NUM_COMBOS as f64 * 100.0
    }

    /// The cells in the order the slider walks them, each flagged as one this
    /// range holds.
    ///
    /// This range's own cells first, strongest first by `ranking`, then every
    /// cell it does not hold, also strongest first. An empty range leaves the
    /// plain ranking, which is what a reader building a range out of an empty
    /// matrix wants.
    pub fn slider_order(&self, ranking: Ranking) -> Vec<(HandClass, bool)> {
        let held = self.class_combo_counts();
        let mine = |class: &HandClass| held[class.index() as usize] > 0.0;
        let order = ranking.order();
        order
            .iter()
            .filter(|class| mine(class))
            .chain(order.iter().filter(|class| !mine(class)))
            .map(|class| (*class, mine(class)))
            .collect()
    }

    /// Where the slider has somewhere to stop, in percent of the deck.
    ///
    /// It moves whole cells, so a position between two of them is one it cannot
    /// express: a handle left there shows one thing and selects another, and a
    /// nudge that lands there moves nothing at all. The edges of the cells are
    /// the stops, and the handles snap to them.
    pub fn slider_stops(&self, ranking: Ranking) -> Vec<f32> {
        let deck = NUM_COMBOS as f64;
        let mut stops = Vec::with_capacity(NUM_CLASSES + 1);
        let mut at = 0.0f64;
        stops.push(0.0);
        for (class, _) in self.slider_order(ranking) {
            at += f64::from(class.combo_count());
            stops.push((at / deck * 100.0) as f32);
        }
        stops
    }

    /// The band `from`..`to` of the ordering this range defines.
    ///
    /// This is what the slider cuts once the matrix holds something. Cutting
    /// the deck instead is what made the slider useless on a chart: the handles
    /// would park at the chart's width, and one press of an arrow key replaced
    /// it with a plain percentile band of the ranking - a hundred and thirty
    /// hands different, for a move of half a point.
    ///
    /// So the ordering is the chart's: its own cells first, strongest first by
    /// `ranking`, and then every cell it does not hold, also strongest first.
    /// The chart is then exactly the band from nought to its own width, which
    /// is where the handles park; one stop either side of that is one matrix
    /// cell, taken off the bottom of the chart or added from the best of what
    /// it folds. Both ends still mean what they always meant - nought leaves
    /// the matrix empty and a hundred leaves no cell out of it.
    ///
    /// Cells the range holds keep the weight they had, so widening a chart
    /// never turns its mixed cells into pure ones. Cells from outside it arrive
    /// whole, there being nothing else they could arrive as.
    ///
    /// An empty range defines no ordering of its own, and this is then the
    /// plain [`Range::window`] over the ranking - which is the right answer for
    /// a reader building a range out of an empty matrix.
    pub fn window_of(&self, from: f64, to: f64, ranking: Ranking) -> Self {
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        let deck = f64::from(NUM_COMBOS as u32);
        let ceiling = (high / 100.0) * deck;
        let floor = (low / 100.0) * deck;
        // A handle sitting exactly on a stop must take the cell that stop is
        // the edge of. The stop reached it as a single-precision percentage, so
        // "exactly" is a thousandth of a hand out either way - room the smallest
        // cell, at four hands, has no trouble clearing.
        let slack = 1e-6 * deck;
        let mut range = Self::empty();
        let mut used = 0.0f64;
        // Whether the cells so far are all inside the part the red handle takes
        // off. The first cell too big to fit under it ends that part, which is
        // the same "at or below the request" rule the blue handle follows.
        let mut trimming = true;
        for (class, mine) in self.slider_order(ranking) {
            let size = f64::from(class.combo_count());
            if used + size > ceiling + slack {
                break;
            }
            if trimming && used + size <= floor + slack {
                used += size;
                continue;
            }
            trimming = false;
            used += size;
            if mine {
                for combo in class.combos() {
                    range.set(combo, self.get(combo));
                }
            } else {
                range.set_class(class, 1.0);
            }
        }
        range
    }

    /// Removes the strongest `percent` of all starting hands from this range.
    ///
    /// This is the negative slider: `top_percent(20)` minus `top_percent(5)` is a
    /// calling range that three-bets its best hands.
    pub fn without_top_percent(&self, percent: f64, ranking: Ranking) -> Self {
        self.difference(&Self::top_percent(percent, ranking))
    }
}

/// The quick buttons under the matrix.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Preset {
    /// Every starting hand.
    All,
    /// Every pocket pair.
    Pairs,
    /// Both cards ten or better.
    Broadway,
    /// Every suited hand.
    Suited,
    /// Queens or better, and ace-king: the narrowest value range anyone opens a
    /// discussion with.
    QqAk,
    /// Tens or better, and ace-queen or better.
    TtAq,
    /// Nines or better, ace-jack or better, and king-queen.
    NineAjKq,
}

impl Preset {
    /// Every preset, in the order the buttons appear.
    pub const ALL: [Preset; 7] = [
        Self::All,
        Self::Pairs,
        Self::Broadway,
        Self::Suited,
        Self::QqAk,
        Self::TtAq,
        Self::NineAjKq,
    ];

    /// A stable identifier for serialisation.
    pub const fn key(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Pairs => "pairs",
            Self::Broadway => "broadway",
            Self::Suited => "suited",
            Self::QqAk => "qq-ak",
            Self::TtAq => "tt-aq",
            Self::NineAjKq => "99-aj-kq",
        }
    }

    /// The button's caption.
    pub const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Pairs => "Pocket",
            Self::Broadway => "Broadway",
            Self::Suited => "Suited",
            // These three say exactly what they select, which is shorter than
            // any name for them and needs no explaining.
            Self::QqAk => "QQ+/AK",
            Self::TtAq => "TT+/AQ+",
            Self::NineAjKq => "99+/AJ+/KQ",
        }
    }

    /// Whether this button selects a shape of the matrix or a range somebody
    /// plays. The two read differently and the panel puts them on separate
    /// lines, which is a property of the preset rather than of its caption.
    pub const fn is_value_range(self) -> bool {
        matches!(self, Self::QqAk | Self::TtAq | Self::NineAjKq)
    }

    /// Reads a [`Preset::key`].
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|p| p.key() == key)
    }

    /// Whether a matrix cell belongs to this preset.
    pub fn holds(self, class: HandClass) -> bool {
        const NINE: u8 = 7;
        const TEN: u8 = 8;
        const JACK: u8 = 9;
        const QUEEN: u8 = 10;
        const KING: u8 = 11;
        let (high, low, pair) = (class.high_rank(), class.low_rank(), class.is_pair());
        match self {
            Self::All => true,
            Self::Pairs => pair,
            Self::Broadway => low >= TEN,
            Self::Suited => class.is_suited(),
            // A pair's two ranks are the same, so the pair arm reads off either.
            Self::QqAk => (pair && high >= QUEEN) || (high == RANK_ACE && low == KING),
            Self::TtAq => (pair && high >= TEN) || (high == RANK_ACE && low >= QUEEN),
            Self::NineAjKq => {
                (pair && high >= NINE)
                    || (high == RANK_ACE && low >= JACK)
                    || (high == KING && low == QUEEN)
            }
        }
    }
}

impl Default for Range {
    fn default() -> Self {
        Self::empty()
    }
}

impl core::fmt::Debug for Range {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Range({})", self.to_notation())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_and_full_are_the_extremes() {
        assert!(Range::empty().is_empty());
        assert_eq!(Range::full().combo_count(), NUM_COMBOS as f64);
        assert!((Range::full().percent_of_deck() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn class_weights_average_the_combos() {
        let mut range = Range::empty();
        let aces = HandClass::parse("AA").unwrap();
        let mut combos = aces.combos();
        range.set(combos.next().unwrap(), 1.0);
        range.set(combos.next().unwrap(), 1.0);
        range.set(combos.next().unwrap(), 1.0);
        assert!((range.class_weight(aces) - 0.5).abs() < 1e-6);
        assert!((range.class_weights()[aces.index() as usize] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn dead_cards_remove_combos() {
        let range = Range::parse("AA").unwrap();
        let dead = CardSet::parse("As").unwrap();
        assert_eq!(range.combo_count(), 6.0);
        assert_eq!(range.combo_count_excluding(dead), 3.0);
        assert_eq!(range.live(dead).len(), 3);
    }

    #[test]
    fn set_operations_behave() {
        let a = Range::parse("TT+").unwrap();
        let b = Range::parse("QQ+").unwrap();
        assert_eq!(a.union(&b).combo_count(), 5.0 * 6.0);
        assert_eq!(a.intersection(&b).combo_count(), 3.0 * 6.0);
        assert_eq!(a.difference(&b).combo_count(), 2.0 * 6.0);
        assert_eq!(a.inverted().combo_count(), (NUM_COMBOS - 30) as f64);
        assert!((a.scaled(0.5).combo_count() - 15.0).abs() < 1e-6);
    }

    #[test]
    fn presets_select_what_their_buttons_promise() {
        assert_eq!(Range::preset(Preset::All).combo_count(), NUM_COMBOS as f64);
        assert_eq!(Range::preset(Preset::Pairs).combo_count(), 13.0 * 6.0);
        assert_eq!(Range::preset(Preset::Suited).combo_count(), 78.0 * 4.0);
        // Broadway is every hand with both cards ten or better: 5 pairs, 10 suited
        // and 10 offsuit cells.
        assert_eq!(
            Range::preset(Preset::Broadway).combo_count(),
            5.0 * 6.0 + 10.0 * 4.0 + 10.0 * 12.0
        );
        // Queens up is three pairs, and ace-king is four suited plus twelve off.
        assert_eq!(Range::preset(Preset::QqAk).combo_count(), 3.0 * 6.0 + 16.0);
        // Tens up, and both ace-queen and ace-king.
        assert_eq!(
            Range::preset(Preset::TtAq).combo_count(),
            5.0 * 6.0 + 2.0 * 16.0
        );
        // Nines up, three ace-highs, and king-queen.
        assert_eq!(
            Range::preset(Preset::NineAjKq).combo_count(),
            6.0 * 6.0 + 3.0 * 16.0 + 16.0
        );
        // What the buttons say is what they select, read back as notation - which
        // spells the suited and offsuit halves out separately.
        assert_eq!(Range::preset(Preset::QqAk).to_notation(), "QQ+,AKs,AKo");
        assert_eq!(Range::preset(Preset::TtAq).to_notation(), "TT+,AQs+,AQo+");
        assert_eq!(
            Range::preset(Preset::NineAjKq).to_notation(),
            "99+,AJs+,KQs,AJo+,KQo"
        );

        for preset in Preset::ALL {
            assert_eq!(Preset::from_key(preset.key()), Some(preset));
        }
    }

    #[test]
    fn cells_report_their_selected_combos() {
        let range = Range::parse("AA,AKs,AKo").unwrap();
        let counts = range.class_combo_counts();
        let at = |hand: &str| counts[HandClass::parse(hand).unwrap().index() as usize];
        assert_eq!(at("AA"), 6.0);
        assert_eq!(at("AKs"), 4.0);
        assert_eq!(at("AKo"), 12.0);
        assert_eq!(at("72o"), 0.0);
    }

    #[test]
    fn the_slider_grows_monotonically() {
        let mut previous = 0.0;
        for percent in [1.0, 5.0, 10.0, 20.0, 40.0, 80.0, 100.0] {
            let range = Range::top_percent(percent, Ranking::EquityVsRandom);
            let count = range.combo_count();
            assert!(count >= previous, "{percent}% shrank the range");
            assert!(
                count <= percent / 100.0 * NUM_COMBOS as f64 + 1e-6,
                "{percent}% overshot"
            );
            previous = count;
        }
        assert_eq!(
            Range::top_percent(100.0, Ranking::EquityVsRandom).combo_count(),
            NUM_COMBOS as f64
        );
        assert!(Range::top_percent(0.0, Ranking::EquityVsRandom).is_empty());
    }

    #[test]
    fn a_window_is_the_band_between_the_handles() {
        let full = Range::window(0.0, 100.0, Ranking::EquityVsRandom);
        assert_eq!(
            full.combo_count(),
            NUM_COMBOS as f64,
            "0 to 100 is everything"
        );

        let band = Range::window(5.0, 20.0, Ranking::EquityVsRandom);
        let wide = Range::top_percent(20.0, Ranking::EquityVsRandom);
        let narrow = Range::top_percent(5.0, Ranking::EquityVsRandom);
        assert!((band.combo_count() - (wide.combo_count() - narrow.combo_count())).abs() < 1e-6);
        assert_eq!(band.intersection(&narrow).combo_count(), 0.0);

        // The handles cannot overtake each other.
        assert_eq!(band, Range::window(20.0, 5.0, Ranking::EquityVsRandom));
        assert!(Range::window(30.0, 30.0, Ranking::EquityVsRandom).is_empty());
    }

    #[test]
    fn the_handles_park_on_what_the_matrix_holds() {
        // Every range is exactly the band from nought to its own width in its
        // own ordering, so parking is exact rather than a best guess.
        let chart = Range::parse("22+,A2s+,K9s+,QTs+,JTs,A8o+,KJo+").unwrap();
        for ranking in Ranking::ALL {
            let width = chart.cell_percent();
            assert_eq!(chart.window_of(0.0, width, ranking), chart, "{ranking:?}");
        }

        // Nought leaves the matrix empty; a hundred leaves no cell out of it.
        let wide = chart.window_of(0.0, 100.0, Ranking::ChenFormula);
        assert!(chart.window_of(0.0, 0.0, Ranking::ChenFormula).is_empty());
        assert_eq!(wide.cell_percent(), 100.0);
        // And what it already had it kept, mixed cells and all.
        for combo in Combo::all() {
            if chart.get(combo) > 0.0 {
                assert_eq!(wide.get(combo), chart.get(combo), "{combo}");
            }
        }

        // An empty matrix has no ordering of its own, so the slider is the
        // plain one it has always been.
        for (low, high) in [(0.0, 20.0), (5.0, 40.0), (0.0, 100.0)] {
            assert_eq!(
                Range::empty().window_of(low, high, Ranking::ChenFormula),
                Range::window(low, high, Ranking::ChenFormula),
            );
        }
    }

    #[test]
    fn one_stop_of_the_slider_is_one_matrix_cell() {
        // The complaint this answers: parked on a chart, the smallest touch of
        // the slider swapped it for a percentile band of the ranking - over a
        // hundred hands different, for a move of half a point.
        let chart = Range::parse("22+,A2s+,K9s+,QTs+,JTs,A8o+,KJo+").unwrap();
        let stops = chart.slider_stops(Ranking::ChenFormula);
        let width = chart.cell_percent();
        let here = stops
            .iter()
            .position(|stop| f64::from(*stop) >= width - 1e-6)
            .expect("the width is a stop");

        let apart = |other: &Range| {
            chart.difference(other).combo_count() + other.difference(&chart).combo_count()
        };
        for (at, what) in [(here + 1, "one cell added"), (here - 1, "one cell dropped")] {
            let moved = chart.window_of(0.0, f64::from(stops[at]), Ranking::ChenFormula);
            let off = apart(&moved);
            assert!(off > 0.0, "{what}: nothing moved");
            assert!(
                off <= 12.0,
                "{what}: {off} hands moved, which is not one cell"
            );
        }

        // Every stop is somewhere the slider can actually express: the band up
        // to it ends exactly there.
        for stop in stops.iter().copied() {
            let band = chart.window_of(0.0, f64::from(stop), Ranking::ChenFormula);
            assert!(
                (band.cell_percent() - f64::from(stop)).abs() < 0.01,
                "a handle on {stop} selects {} of the deck",
                band.cell_percent()
            );
        }
    }

    #[test]
    fn the_negative_slider_carves_out_the_top() {
        let wide = Range::top_percent(20.0, Ranking::EquityVsRandom);
        let narrow = Range::top_percent(5.0, Ranking::EquityVsRandom);
        let call = wide.without_top_percent(5.0, Ranking::EquityVsRandom);
        assert!((call.combo_count() - (wide.combo_count() - narrow.combo_count())).abs() < 1e-6);
        assert_eq!(call.intersection(&narrow).combo_count(), 0.0);
    }
}
