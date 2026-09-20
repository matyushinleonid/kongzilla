//! WebAssembly bindings.
//!
//! The boundary is deliberately narrow. Mutations are small method calls, and the
//! interface reads everything it needs back through [`Engine::view`] - one JSON
//! document per change - so a redraw never pulls 1326-element arrays across the
//! boundary. The one exception is [`Engine::highlight`], which returns 169 floats
//! as a `Float32Array` because it runs on every mouse move.

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use kongzilla_core::breakdown::{Breakdown, BreakdownMode};
use kongzilla_core::cards::{Card, CardSet, Combo, HandClass, NUM_CLASSES};
use kongzilla_core::engine::{Session, Snapshot};
use kongzilla_core::equity::EquityReport;
use kongzilla_core::groups::{colour_from_key, colour_key, PALETTE};
use kongzilla_core::library::{charts, Actions, Seat, Stack};
use kongzilla_core::range::Preset;
use kongzilla_core::ranking::Ranking;
use kongzilla_core::stats::{ClassifyOptions, StatBlock, StatId, DEFS};
use serde::Serialize;
use wasm_bindgen::prelude::*;

/// One statistic, as the interface needs to draw it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct StatDefView {
    index: u8,
    key: &'static str,
    label: &'static str,
    block: &'static str,
    optional: bool,
}

/// One chart in the preflop library.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ChartView {
    id: &'static str,
    stack: &'static str,
    /// What the chart is a strategy for.
    spot: &'static str,
    /// Whose strategy it is.
    seat: &'static str,
    seat_label: &'static str,
    /// The seat being answered, where there is one: the opener a three-bet is
    /// over, or the raise a defence is against.
    versus: Option<&'static str>,
    versus_label: Option<&'static str>,
    /// The chip's caption.
    label: &'static str,
    description: String,
    percent: f64,
    /// What the solver actually does in this spot, as key, name and how much of
    /// the deck it does it with. The panel puts a switch against each.
    actions: Vec<(&'static str, &'static str, f64)>,
    size_bb: f32,
    /// Whether any of what this chart plays is played at no gain, so the
    /// panel knows whether excluding those hands would do anything at all.
    has_zero_ev: bool,
}

/// One seat's tab.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct PlayerView {
    name: String,
    /// The one hand this seat holds, if it holds exactly one.
    ///
    /// A seat holding one hand is that hand, and is named as it everywhere -
    /// "A♠K♠" is what the reader put there and "Range B" hides it behind a
    /// label that says nothing.
    hand: Option<String>,
    notation: String,
    combos: f64,
    percent: f64,
    /// Mean weight per matrix cell, for the seat's thumbnail.
    class_weights: Vec<f32>,
    /// Where the seat's range slider sits, in percent of the deck.
    slider_low: f64,
    slider_high: f64,
    /// The library chart this seat was loaded from, and whether what is in the
    /// matrix is still that chart rather than an edit of it.
    chart: Option<String>,
    chart_edited: bool,
}

/// One combination of one matrix cell, as the popup draws it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CellComboView {
    index: u16,
    name: String,
    weight: f32,
    colour: &'static str,
    passing: f32,
    matches: bool,
    dealt: bool,
}

/// The strongest slice of a range, as the panel reports it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct CutView {
    share: f64,
    covered: f64,
    threshold: f64,
    board: String,
    combos: f64,
}

/// Everything a redraw needs.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct View {
    board: String,
    board_cards: Vec<String>,
    street: &'static str,
    dead: Vec<String>,
    /// The one hand the active seat holds, if it holds exactly one.
    hand: Option<String>,
    active: usize,
    players: Vec<PlayerView>,
    class_weights: Vec<f32>,
    class_combos: Vec<f32>,
    breakdown: Breakdown,
    /// One marker per statistic: a colour key, "mixed" for a gear, or "empty".
    marks: Vec<&'static str>,
    /// Per-cell colour, as the share of the cell's weight in each colour.
    class_colours: Vec<Vec<f32>>,
    /// The weight of the range in each colour, unpainted first.
    group_shares: Vec<f64>,
    /// How many colours are in use, so one colour needs no key to tell apart.
    colours_used: usize,
    /// The palette colour currently held.
    colour: &'static str,
    /// Combos per cell that survive the applied street filters, `0..=1`.
    class_passing: Vec<f32>,
    /// The suits a suited cell is holding, when it is not holding all of them.
    class_suits: Vec<String>,
    checkmarks: Vec<bool>,
    /// The groups of flops a pass over them is narrowed to, as `axis/group`
    /// keys. Empty means every flop, which is the ordinary case.
    flop_groups: Vec<String>,
    /// How many flops a pass would look at, after the dead cards and the groups.
    filtered_flops: u64,
    /// Which streets have had their filter pressed.
    streets_on: Vec<bool>,
    /// How many combos are left after the filters up to and including a street.
    street_counts: Vec<f64>,
    /// What a street filter would leave if it were pressed now.
    would_pass: f64,
    /// How many street buttons the board has room for.
    streets_dealt: usize,
    filters_enabled: bool,
    pass_fraction: f64,
    live_combos: f64,
    mode: BreakdownMode,
    ranking: &'static str,
    options: ClassifyOptions,
    equity: Option<EquityReport>,
    /// Which seats the equity report is about, in the order it lists them.
    equity_seats: Vec<usize>,
    /// Which seat the statistics panel is comparing against, if any.
    compare_seat: Option<usize>,
    /// Which seat the per-hand equity views measure against, or the whole field.
    versus_seat: Option<usize>,
    /// Whether the selected seat is something the reader can change. A hand has
    /// been dealt, so it is not.
    editable: bool,
    /// The cards the seats have been dealt, which nobody else can hold.
    dealt: Vec<String>,
    /// What share of each colour the street filters let through, unpainted first.
    colour_shares: Vec<f32>,
    /// How many seats the table will hold.
    max_seats: usize,
    effective_notation: String,
    effective_combos: f64,
}

/// A live analysis session.
#[wasm_bindgen]
pub struct Engine {
    session: Session,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

#[wasm_bindgen]
impl Engine {
    /// Creates an empty session with two seats.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine {
            session: Session::new(),
        }
    }

    /// The statistic registry, as JSON. Fetched once at start-up.
    #[wasm_bindgen(js_name = statDefinitions)]
    pub fn stat_definitions() -> String {
        let defs: Vec<StatDefView> = DEFS
            .iter()
            .map(|d| StatDefView {
                index: d.id.index(),
                key: d.key,
                label: d.label,
                block: d.block.key(),
                optional: d.optional,
            })
            .collect();
        to_json(&defs)
    }

    /// The block headings, as JSON.
    #[wasm_bindgen(js_name = blockLabels)]
    pub fn block_labels() -> String {
        let blocks: Vec<(&str, &str)> = StatBlock::ALL
            .iter()
            .map(|b| (b.key(), b.label()))
            .collect();
        to_json(&blocks)
    }

    /// The available slider orderings, as JSON.
    #[wasm_bindgen(js_name = rankings)]
    pub fn rankings() -> String {
        let all: Vec<(&str, &str)> = Ranking::ALL.iter().map(|r| (r.key(), r.label())).collect();
        to_json(&all)
    }

    // ----- board ----------------------------------------------------------------

    /// Replaces the board from text such as `Kc Qh Jh`.
    #[wasm_bindgen(js_name = setBoard)]
    pub fn set_board(&mut self, text: &str) -> Result<(), JsError> {
        self.session.set_board_text(text).map_err(to_error)
    }

    /// Adds one card to the board.
    #[wasm_bindgen(js_name = pushBoardCard)]
    pub fn push_board_card(&mut self, text: &str) -> Result<bool, JsError> {
        let card = Card::parse(text).map_err(to_error)?;
        Ok(self.session.push_board_card(card))
    }

    /// Removes the last board card.
    #[wasm_bindgen(js_name = popBoardCard)]
    pub fn pop_board_card(&mut self) {
        self.session.pop_board_card();
    }

    /// Shows the board up to `len` cards.
    #[wasm_bindgen(js_name = truncateBoard)]
    pub fn truncate_board(&mut self, len: usize) {
        self.session.truncate_board(len);
    }

    /// Adds or removes a dead card.
    #[wasm_bindgen(js_name = toggleDead)]
    pub fn toggle_dead(&mut self, text: &str) -> Result<(), JsError> {
        let card = Card::parse(text).map_err(to_error)?;
        self.session.toggle_dead(card);
        Ok(())
    }

    /// Replaces the dead cards from text.
    #[wasm_bindgen(js_name = setDead)]
    pub fn set_dead(&mut self, text: &str) -> Result<(), JsError> {
        let set = CardSet::parse(text).map_err(to_error)?;
        self.session.set_dead(set);
        Ok(())
    }

    /// Which cards cannot be dealt, as JSON.
    #[wasm_bindgen(js_name = usedCards)]
    pub fn used_cards(&self) -> String {
        let used: Vec<String> = self
            .session
            .board()
            .mask()
            .union(self.session.dead())
            .iter()
            .map(|c| c.to_string())
            .collect();
        to_json(&used)
    }

    // ----- ranges ---------------------------------------------------------------

    /// Selects a seat.
    #[wasm_bindgen(js_name = setActive)]
    pub fn set_active(&mut self, index: usize) {
        self.session.set_active(index);
    }

    /// Adds a seat holding one known hand, and says whether it was dealt.
    ///
    /// Refuses a hand needing a card that is already on the board, in the dead
    /// cards or in another hand.
    #[wasm_bindgen(js_name = addHand)]
    pub fn add_hand(&mut self, hand: &str) -> bool {
        match Combo::parse(hand) {
            Ok(combo) => self.session.add_hand(combo).is_some(),
            Err(_) => false,
        }
    }

    /// Adds a seat, and says whether there was room for one.
    #[wasm_bindgen(js_name = addSeat)]
    pub fn add_seat(&mut self) -> bool {
        self.session.add_seat().is_some()
    }

    /// Removes a seat, and says whether it went. Two always stay.
    #[wasm_bindgen(js_name = removeSeat)]
    pub fn remove_seat(&mut self, index: usize) -> bool {
        self.session.remove_seat(index)
    }

    /// Replaces the active range from text.
    #[wasm_bindgen(js_name = setRangeText)]
    pub fn set_range_text(&mut self, text: &str) -> Result<(), JsError> {
        self.session.set_active_range_text(text).map_err(to_error)
    }

    /// Replaces the active range with the strongest `percent` of hands.
    #[wasm_bindgen(js_name = setTopPercent)]
    pub fn set_top_percent(&mut self, percent: f64) {
        self.session.set_active_top_percent(percent);
    }

    /// Replaces the active range with the band between the slider handles.
    #[wasm_bindgen(js_name = setWindow)]
    pub fn set_window(&mut self, from: f64, to: f64) {
        self.session.set_active_window(from, to);
    }

    /// Removes the strongest `percent` of hands from the active range.
    #[wasm_bindgen(js_name = removeTopPercent)]
    pub fn remove_top_percent(&mut self, percent: f64) {
        self.session.remove_active_top_percent(percent);
    }

    /// Sets one matrix cell on the active range.
    #[wasm_bindgen(js_name = setClassWeight)]
    pub fn set_class_weight(&mut self, class: u8, weight: f32) {
        if (class as usize) < NUM_CLASSES {
            self.session
                .set_active_class(HandClass::from_index(class), weight);
        }
    }

    /// The preflop library, as JSON. Fetched once at start-up.
    ///
    /// Charts arrive in two rows: what a seat opens with, and what the big blind
    /// continues with against it.
    #[wasm_bindgen(js_name = library)]
    pub fn library() -> String {
        // Each chip carries which game it belongs to and what it is, because a
        // raked cash game is not another depth of a tournament and the two
        // cannot sit under one heading.
        let stacks: Vec<(&str, &str, &str)> = Stack::ALL
            .iter()
            .map(|s| (s.key(), s.description(), s.game()))
            .collect();
        // The seats in the order they act, so a row of them can be shown in it:
        // the charts arrive grouped by how they were shot rather than by who is
        // sitting where.
        let seats: Vec<(&str, &str)> = Seat::ALL.iter().map(|s| (s.key(), s.label())).collect();
        let rows: Vec<(&str, &str)> = vec![
            ("open", "Open"),
            ("defend", "BB vs"),
            ("facing", "Facing open"),
        ];
        let entries: Vec<ChartView> = charts()
            .iter()
            .map(|chart| ChartView {
                id: chart.id,
                stack: chart.stack.key(),
                spot: chart.spot.key(),
                seat: chart.seat.key(),
                seat_label: chart.seat.label(),
                actions: chart
                    .offers()
                    .into_iter()
                    .map(|(action, label, percent)| (action.key(), label, percent))
                    .collect(),
                versus: chart.versus.map(Seat::key),
                versus_label: chart.versus.map(Seat::label),
                label: chart.label(),
                description: chart.description(),
                has_zero_ev: chart.has_zero_ev(),
                percent: chart.percent(),
                size_bb: chart.size_bb,
            })
            .collect();
        to_json(&(stacks, rows, entries, seats))
    }

    /// Replaces the active range with one from the preflop library.
    ///
    /// `actions` names which of what the solver does in that spot to take, as
    /// a comma-separated list - "call,raise" for everything a seat carries on
    /// with, "raise" for the three-bet alone. Anything unrecognised is ignored,
    /// so an empty list asks for nothing and gets it.
    #[wasm_bindgen(js_name = loadLibrary)]
    pub fn load_library(&mut self, id: &str, actions: &str, without_zero_ev: bool) -> bool {
        self.session
            .load_library_chart(id, Actions::parse(actions), without_zero_ev)
    }

    /// Adds everything a quick button selects to the active range.
    #[wasm_bindgen(js_name = addPreset)]
    pub fn add_preset(&mut self, key: &str) {
        if let Some(preset) = Preset::from_key(key) {
            self.session.add_preset(preset);
        }
    }

    /// Replaces the active range with one chart less another, weight by weight.
    ///
    /// Not reached from the interface; see `Session::load_library_less`.
    #[wasm_bindgen(js_name = loadLibraryLess)]
    pub fn load_library_less(&mut self, id: &str, minus: &str, without_zero_ev: bool) -> bool {
        self.session.load_library_less(id, minus, without_zero_ev)
    }

    /// Takes a chart away from the active range, weight by weight.
    ///
    /// Not reached from the interface; see `Session::subtract_library_chart`.
    #[wasm_bindgen(js_name = subtractLibrary)]
    pub fn subtract_library(&mut self, id: &str, without_zero_ev: bool) -> bool {
        self.session.subtract_library_chart(id, without_zero_ev)
    }

    /// Takes everything a quick button selects away from the active range.
    ///
    /// Not reached from the interface; see `Session::subtract_library_chart`.
    #[wasm_bindgen(js_name = subtractPreset)]
    pub fn subtract_preset(&mut self, key: &str) {
        if let Some(preset) = Preset::from_key(key) {
            self.session.subtract_preset(preset);
        }
    }

    /// The quick buttons, as JSON: key, caption, and whether it is a range
    /// somebody plays rather than a shape of the matrix.
    #[wasm_bindgen(js_name = presets)]
    pub fn presets() -> String {
        let all: Vec<(&str, &str, bool)> = Preset::ALL
            .iter()
            .map(|p| (p.key(), p.label(), p.is_value_range()))
            .collect();
        to_json(&all)
    }

    /// Empties the active range.
    #[wasm_bindgen(js_name = clearRange)]
    pub fn clear_range(&mut self) {
        self.session.clear_active_range();
    }

    /// Chooses the slider ordering.
    #[wasm_bindgen(js_name = setRanking)]
    pub fn set_ranking(&mut self, key: &str) {
        if let Some(ranking) = Ranking::from_key(key) {
            self.session.set_ranking(ranking);
        }
    }

    /// The matrix cell labels, in index order, as JSON.
    #[wasm_bindgen(js_name = classLabels)]
    pub fn class_labels() -> String {
        let labels: Vec<String> = HandClass::all().map(|c| c.to_string()).collect();
        to_json(&labels)
    }

    // ----- filters and options ---------------------------------------------------

    /// The palette, in the order it is offered.
    #[wasm_bindgen(js_name = palette)]
    pub fn palette() -> String {
        to_json(&PALETTE)
    }

    /// The colour the palette is holding.
    #[wasm_bindgen(js_name = colour)]
    pub fn colour(&self) -> String {
        colour_key(self.session.colour()).to_owned()
    }

    /// Picks the colour to paint with.
    #[wasm_bindgen(js_name = setColour)]
    pub fn set_colour(&mut self, colour: &str) {
        if let Some(colour) = colour_from_key(colour) {
            self.session.set_colour(colour);
        }
    }

    /// Paints every hand in the range that carries a statistic.
    #[wasm_bindgen(js_name = paintStat)]
    pub fn paint_stat(&mut self, stat: u8, colour: &str) {
        if let (Some(stat), Some(colour)) = (StatId::from_index(stat), colour_from_key(colour)) {
            self.session.paint_stat(stat, colour);
        }
    }

    /// Paints one hand.
    #[wasm_bindgen(js_name = paintCombo)]
    pub fn paint_combo(&mut self, combo: u16, colour: &str) {
        if let Some(colour) = colour_from_key(colour) {
            self.session.paint_combo(Combo::from_index(combo), colour);
        }
    }

    /// The colour of one hand.
    #[wasm_bindgen(js_name = comboColour)]
    pub fn combo_colour(&self, combo: u16) -> String {
        colour_key(self.session.combo_colour(Combo::from_index(combo))).to_owned()
    }

    /// Chooses the seat the per-hand equity views measure against. Out of range
    /// means the whole field at once.
    #[wasm_bindgen(js_name = setVersusSeat)]
    pub fn set_versus_seat(&mut self, seat: Option<usize>) {
        self.session.set_versus_seat(seat);
    }

    /// The headline of the last pass over all the flops, re-read for the
    /// checkmarks as they stand - or `null` when the pass no longer applies.
    #[wasm_bindgen(js_name = preflopHit)]
    pub fn preflop_hit(&self) -> Option<f64> {
        self.session.preflop_hit()
    }

    /// Swaps painted for unpainted hand by hand, so the two halves add up to
    /// the range.
    #[wasm_bindgen(js_name = invertGroups)]
    pub fn invert_groups(&mut self) {
        self.session.invert_groups();
    }

    /// Swaps which categories are painted, leaving the hands where they fall.
    #[wasm_bindgen(js_name = invertCategories)]
    pub fn invert_categories(&mut self) {
        self.session.invert_categories();
    }

    /// Unpaints every hand, leaving the street filters alone.
    #[wasm_bindgen(js_name = clearGroups)]
    pub fn clear_groups(&mut self) {
        self.session.clear_groups();
    }

    /// Puts the default grouping back.
    #[wasm_bindgen(js_name = resetGroups)]
    pub fn reset_groups(&mut self) {
        self.session.reset_groups();
    }

    /// Paints every hand that carries both statistics at once.
    #[wasm_bindgen(js_name = paintIntersection)]
    pub fn paint_intersection(&mut self, a: u8, b: u8, colour: &str) {
        if let (Some(a), Some(b), Some(colour)) = (
            StatId::from_index(a),
            StatId::from_index(b),
            colour_from_key(colour),
        ) {
            self.session.paint_intersection(a, b, colour);
        }
    }

    /// The statistics panel as text, restricted to one statistic when given.
    #[wasm_bindgen(js_name = statisticsText)]
    pub fn statistics_text(&self, within: Option<u8>) -> String {
        self.session
            .statistics_text(within.and_then(StatId::from_index))
    }

    /// Every combination the statistics panel is speaking about, as notation.
    #[wasm_bindgen(js_name = statisticsCombos)]
    pub fn statistics_combos(&self, within: Option<u8>) -> String {
        self.session
            .statistics_combos(within.and_then(StatId::from_index))
    }

    /// Freezes the painted range as the filter for one street, or lifts it.
    #[wasm_bindgen(js_name = toggleStreetFilter)]
    pub fn toggle_street_filter(&mut self, street: usize) -> bool {
        self.session.toggle_street_filter(street)
    }

    /// Keeps the strongest `share` of the range, by equity on this board.
    ///
    /// Returns the cut as JSON, or `null` preflop, where per-combo equity is
    /// sampled and far too noisy to cut on.
    #[wasm_bindgen(js_name = setContinueByEquity)]
    pub fn set_continue_by_equity(&mut self, share: f64) -> String {
        let cut = self
            .session
            .set_continue_by_equity(share)
            .map(|cut| CutView {
                share: cut.share,
                covered: cut.covered,
                threshold: f64::from(cut.threshold),
                board: cut.board.clone(),
                combos: cut.range.combo_count(),
            });
        to_json(&cut)
    }

    /// Drops the equity cut, leaving the statistic filters alone.
    #[wasm_bindgen(js_name = clearCut)]
    pub fn clear_cut(&mut self) {
        self.session.clear_cut();
    }

    /// How much of each matrix cell the cut keeps, for painting the selection.
    #[wasm_bindgen(js_name = cutShares)]
    pub fn cut_shares(&self) -> Option<Vec<f32>> {
        self.session.cut_shares().map(|shares| shares.to_vec())
    }

    /// What one combo is on this board, as JSON.
    #[wasm_bindgen(js_name = describeCombo)]
    pub fn describe_combo(&self, combo: u16) -> String {
        if (combo as usize) >= kongzilla_core::cards::NUM_COMBOS {
            return "[]".to_owned();
        }
        to_json(&self.session.describe_combo(Combo::from_index(combo)))
    }

    /// Clears every filter on the active seat.
    #[wasm_bindgen(js_name = clearFilters)]
    pub fn clear_filters(&mut self) {
        self.session.clear_filters();
    }

    /// The hands carrying one statistic, with their colours.
    #[wasm_bindgen(js_name = statCombos)]
    pub fn stat_combos(&self, stat: u8) -> String {
        let Some(stat) = StatId::from_index(stat) else {
            return "[]".to_owned();
        };
        let session = &self.session;
        let combos: Vec<(u16, String, String)> = session
            .stats()
            .live()
            .iter()
            .filter(|combo| {
                session.active().range.get(*combo) > 0.0 && session.stats().mask(*combo).has(stat)
            })
            .map(|combo| {
                (
                    combo.index(),
                    combo.to_string(),
                    colour_key(session.combo_colour(combo)).to_owned(),
                )
            })
            .collect();
        to_json(&combos)
    }

    /// Everything one matrix cell holds, combination by combination.
    ///
    /// `within` restricts the `matches` flag to one statistic, which is what the
    /// popup uses to light the hands a hovered row is about.
    #[wasm_bindgen(js_name = cellCombos)]
    pub fn cell_combos(&self, class: u8, within: Option<u8>) -> String {
        let class = HandClass::from_index(class);
        let within = within.and_then(StatId::from_index);
        let combos: Vec<CellComboView> = self
            .session
            .cell_combos(class, within)
            .into_iter()
            .map(|entry| CellComboView {
                index: entry.combo.index(),
                name: entry.combo.to_string(),
                weight: entry.weight,
                colour: colour_key(entry.colour),
                passing: entry.passing,
                matches: entry.matches,
                dealt: entry.dealt,
            })
            .collect();
        to_json(&combos)
    }

    /// The colours in the active cell, for the suit strip and the popup.
    #[wasm_bindgen(js_name = comboColours)]
    pub fn combo_colours(&self, class: u8) -> String {
        let class = HandClass::from_index(class);
        let combos: Vec<(u16, String, String)> = class
            .combos()
            .map(|combo| {
                (
                    combo.index(),
                    combo.to_string(),
                    colour_key(self.session.combo_colour(combo)).to_owned(),
                )
            })
            .collect();
        to_json(&combos)
    }

    /// Chooses absolute or cumulative reporting.
    #[wasm_bindgen(js_name = setMode)]
    pub fn set_mode(&mut self, mode: &str) {
        self.session.set_mode(match mode {
            "cumulative" => BreakdownMode::Cumulative,
            _ => BreakdownMode::Absolute,
        });
    }

    /// Turns the one-card backdoor flushdraw statistics on or off.
    #[wasm_bindgen(js_name = setOneCardBackdoorFlushdraw)]
    pub fn set_one_card_backdoor_flushdraw(&mut self, enabled: bool) {
        self.session.set_options(ClassifyOptions {
            one_card_backdoor_flushdraw: enabled,
        });
    }

    // ----- output ----------------------------------------------------------------

    /// Sets the weight of one combo, for suit-level editing.
    #[wasm_bindgen(js_name = setComboWeight)]
    pub fn set_combo_weight(&mut self, combo: u16, weight: f32) {
        if (combo as usize) < kongzilla_core::cards::NUM_COMBOS {
            self.session
                .set_combo_weight(Combo::from_index(combo), weight);
        }
    }

    /// Where the top-of-the-range slider has anywhere to stop, as shares.
    #[wasm_bindgen(js_name = equitySteps)]
    pub fn equity_steps(&self) -> Vec<f32> {
        self.session.equity_steps()
    }

    /// Which statistics one matrix cell is about, as a share of the cell.
    ///
    /// Empty when the board and the dead cards have taken every combination of
    /// it away, which is the one case with no answer rather than a row of
    /// noughts.
    #[wasm_bindgen(js_name = classStats)]
    pub fn class_stats(&self, index: u8) -> Vec<f32> {
        self.session.class_stats(HandClass::from_index(index))
    }

    /// The same for one combination, where every share is nought or one.
    #[wasm_bindgen(js_name = comboStats)]
    pub fn combo_stats(&self, index: u16) -> Vec<f32> {
        self.session.combo_stats(Combo::from_index(index))
    }

    /// The individual combos of one matrix cell, as JSON.
    #[wasm_bindgen(js_name = classCombos)]
    pub fn class_combos(&self, class: u8) -> String {
        if (class as usize) >= NUM_CLASSES {
            return "[]".to_owned();
        }
        let combos: Vec<(u16, String, f32)> = self
            .session
            .class_combos(HandClass::from_index(class))
            .into_iter()
            .map(|(combo, weight)| (combo.index(), combo.to_string(), weight))
            .collect();
        to_json(&combos)
    }

    /// Adds or removes a preflop checkmark.
    #[wasm_bindgen(js_name = toggleCheckmark)]
    pub fn toggle_checkmark(&mut self, stat: u8) {
        if let Some(stat) = StatId::from_index(stat) {
            self.session.toggle_checkmark(stat);
        }
    }

    /// Clears every preflop checkmark.
    #[wasm_bindgen(js_name = clearCheckmarks)]
    pub fn clear_checkmarks(&mut self) {
        self.session.clear_checkmarks();
    }

    /// How often each statistic comes with each other one, as JSON.
    pub fn overlap(&self) -> String {
        to_json(&self.session.overlap())
    }

    /// The active range classified against every flop, as JSON.
    ///
    /// Not instant for a wide range - tens of millions of classifications - so
    /// the interface runs it on request.
    pub fn preflop(&self) -> String {
        to_json(&self.session.preflop())
    }

    /// The pass for the seat as it stands, as JSON, or `null` if none was run.
    ///
    /// The difference from `preflop` is that this never does the work: it says
    /// what is already known, so moving between seats can put back the answer
    /// each one already has.
    #[wasm_bindgen(js_name = preflopCached)]
    pub fn preflop_cached(&self) -> Option<String> {
        self.session.preflop_cached().map(|pass| to_json(&pass))
    }

    /// Where the range slider's handles have somewhere to stop.
    ///
    /// The edges of the matrix cells, in the order the slider walks them, as
    /// percentages of the deck. Between two of them there is nothing to choose.
    #[wasm_bindgen(js_name = sliderStops)]
    pub fn slider_stops(&self) -> Vec<f32> {
        self.session.slider_stops()
    }

    /// Whether the pass over the flops has left per-hand equity standing.
    #[wasm_bindgen(js_name = preflopEquityReady)]
    pub fn preflop_equity_ready(&self) -> bool {
        self.session.preflop_equity_ready()
    }

    /// How many hands the equity riding along with a pass would evaluate.
    ///
    /// Nought when there is nothing to measure against, which is how the panel
    /// knows the pass is only a pass.
    #[wasm_bindgen(js_name = preflopEquityWork)]
    pub fn preflop_equity_work(&self) -> f64 {
        self.session.preflop_equity_work()
    }

    /// Adds or removes one group of flops from what a pass looks at.
    #[wasm_bindgen(js_name = toggleFlopGroup)]
    pub fn toggle_flop_group(&mut self, axis: &str, group: &str) -> bool {
        self.session.toggle_flop_group(axis, group)
    }

    /// Puts every flop back into what a pass looks at.
    #[wasm_bindgen(js_name = clearFlopFilter)]
    pub fn clear_flop_filter(&mut self) {
        self.session.clear_flop_filter();
    }

    /// How often each kind of flop comes, as JSON.
    #[wasm_bindgen(js_name = flopBreakdown)]
    pub fn flop_breakdown(&self) -> String {
        to_json(&self.session.flop_breakdown())
    }

    /// How each remaining card changes the hand's equity, as JSON, or `null`.
    pub fn hotness(&self) -> String {
        to_json(&self.session.hotness())
    }

    /// Equity per combo index, `-1.0` where the combo is not in the range.
    #[wasm_bindgen(js_name = equityByCombo)]
    pub fn equity_by_combo(&self) -> Vec<f32> {
        self.session
            .equity_by_combo()
            .map(|result| result.equity)
            .unwrap_or_default()
    }

    /// Weight per combo index, alongside [`Engine::equity_by_combo`].
    #[wasm_bindgen(js_name = comboWeights)]
    pub fn combo_weights(&self) -> Vec<f32> {
        self.session
            .equity_by_combo()
            .map(|result| result.weight)
            .unwrap_or_default()
    }

    /// Outright wins per combo, alongside [`Engine::equity_by_combo`].
    #[wasm_bindgen(js_name = comboWins)]
    pub fn combo_wins(&self) -> Vec<f32> {
        self.session
            .equity_by_combo()
            .map(|result| result.win)
            .unwrap_or_default()
    }

    /// Splits per combo, alongside [`Engine::equity_by_combo`].
    #[wasm_bindgen(js_name = comboTies)]
    pub fn combo_ties(&self) -> Vec<f32> {
        self.session
            .equity_by_combo()
            .map(|result| result.tie)
            .unwrap_or_default()
    }

    /// Deals a random flop from the ones the ticked groups leave.
    #[wasm_bindgen(js_name = dealFlop)]
    pub fn deal_flop(&mut self) -> bool {
        self.session.deal_flop()
    }

    /// Deals a random flop from one bucket of the flop breakdown.
    #[wasm_bindgen(js_name = dealFlopFrom)]
    pub fn deal_flop_from(&mut self, axis: &str, group: &str) -> bool {
        self.session.deal_flop_from(axis, group)
    }

    /// Equity per combo for the seat the active range is measured against.
    ///
    /// Empty when there is no distinct opponent to draw.
    #[wasm_bindgen(js_name = opponentEquityByCombo)]
    pub fn opponent_equity_by_combo(&self) -> Vec<f32> {
        self.session
            .opponent_equity_by_combo()
            .map(|result| result.equity)
            .unwrap_or_default()
    }

    /// Weight per combo index, alongside [`Engine::opponent_equity_by_combo`].
    #[wasm_bindgen(js_name = opponentComboWeights)]
    pub fn opponent_combo_weights(&self) -> Vec<f32> {
        self.session
            .opponent_equity_by_combo()
            .map(|result| result.weight)
            .unwrap_or_default()
    }

    /// Everything a redraw needs, as JSON.
    pub fn view(&self) -> String {
        let session = &self.session;
        let effective = session.effective_range();
        let view = View {
            board: session.board().to_string(),
            board_cards: session.board().cards().map(|c| c.to_string()).collect(),
            street: session.board().street().label(),
            dead: session.dead().iter().map(|c| c.to_string()).collect(),
            hand: session.lone_combo().map(|combo| combo.to_string()),
            active: session.active_index(),
            players: session
                .players()
                .iter()
                .enumerate()
                .map(|(seat, p)| PlayerView {
                    name: p.name.clone(),
                    hand: session.lone_combo_for(seat).map(|combo| combo.to_string()),
                    notation: p.range.to_notation(),
                    combos: p.range.combo_count(),
                    percent: p.range.percent_of_deck() * 100.0,
                    class_weights: p.range.class_weights().to_vec(),
                    slider_low: p.slider.low,
                    slider_high: p.slider.high,
                    chart: p.from_library.as_ref().map(|from| from.id.clone()),
                    chart_edited: p
                        .from_library
                        .as_ref()
                        .is_some_and(|from| from.pristine != p.range),
                })
                .collect(),
            class_weights: session.active().range.class_weights().to_vec(),
            class_combos: session.active().range.class_combo_counts().to_vec(),
            breakdown: session.breakdown(),
            marks: StatId::all().map(|s| session.mark(s).key()).collect(),
            class_colours: session.class_colours().iter().map(|c| c.to_vec()).collect(),
            group_shares: session.group_shares().to_vec(),
            colours_used: session.colours_used(),
            colour: colour_key(session.colour()),
            class_passing: session.class_passing().to_vec(),
            class_suits: session
                .class_suits()
                .iter()
                .map(|suits| String::from_utf8_lossy(suits).into_owned())
                .collect(),
            checkmarks: StatId::all().map(|s| session.checkmarks().has(s)).collect(),
            flop_groups: session
                .flop_filter()
                .selected()
                .into_iter()
                .map(|(axis, group)| format!("{axis}/{group}"))
                .collect(),
            filtered_flops: session.flop_filter_count(),
            streets_on: (0..3)
                .map(|street| session.street_applied(street))
                .collect(),
            street_counts: (0..3).map(|street| session.street_count(street)).collect(),
            would_pass: session.painted_combos(),
            streets_dealt: session.streets_dealt(),
            filters_enabled: session.filters_enabled(),
            pass_fraction: session.pass_fraction(),
            live_combos: session.live_combos(),
            mode: session.mode(),
            ranking: session.ranking().key(),
            options: session.options(),
            equity: session.equity(),
            equity_seats: session.equity_seats(),
            compare_seat: session.compare_seat(),
            versus_seat: session.versus_seat(),
            editable: session.editable(),
            dealt: session
                .dealt_hands()
                .iter()
                .map(|c| c.to_string())
                .collect(),
            colour_shares: session.colour_shares().to_vec(),
            max_seats: Session::MAX_SEATS,
            effective_notation: effective.to_notation(),
            effective_combos: effective.combo_count(),
        };
        to_json(&view)
    }

    /// The second statistics column, for the seat being compared against, as
    /// JSON - or `null` when no seat is. Takes the same statistic restriction
    /// the first column is under, so the two columns are always about the same
    /// question.
    #[wasm_bindgen(js_name = compareBreakdown)]
    pub fn compare_breakdown(&self, within: Option<u8>) -> String {
        let mask = within.and_then(StatId::from_index).map(StatId::mask);
        to_json(&self.session.compare_breakdown(mask))
    }

    /// Sets what share of a colour the street filters let through, `0..=1`.
    #[wasm_bindgen(js_name = setColourShare)]
    pub fn set_colour_share(&mut self, colour: &str, share: f64) {
        if let Some(colour) = colour_from_key(colour) {
            self.session.set_colour_share(colour, share);
        }
    }

    /// Chooses the seat the statistics panel compares against. Out of range
    /// clears it.
    #[wasm_bindgen(js_name = setCompareSeat)]
    pub fn set_compare_seat(&mut self, seat: Option<usize>) {
        self.session.set_compare_seat(seat);
    }

    /// The statistics panel restricted to one statistic, as hovering does.
    #[wasm_bindgen(js_name = breakdownWithin)]
    pub fn breakdown_within(&self, stat: u8) -> String {
        match StatId::from_index(stat) {
            Some(stat) => to_json(&self.session.breakdown_within(stat.mask())),
            None => to_json(&self.session.breakdown()),
        }
    }

    /// Per-cell matrix weights for one statistic, or the whole range when `stat`
    /// is out of range.
    pub fn highlight(&self, stat: u8) -> Vec<f32> {
        let mask = StatId::from_index(stat)
            .map(StatId::mask)
            .unwrap_or_default();
        self.session.highlight(mask).to_vec()
    }

    /// The session as JSON, for saving and for shareable links.
    pub fn snapshot(&self) -> String {
        to_json(&self.session.snapshot())
    }

    /// Restores a session from [`Engine::snapshot`] output.
    pub fn restore(&mut self, json: &str) -> Result<(), JsError> {
        let snapshot: Snapshot =
            serde_json::from_str(json).map_err(|e| JsError::new(&e.to_string()))?;
        self.session = Session::restore(&snapshot).map_err(to_error)?;
        Ok(())
    }
}

fn to_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn to_error(error: kongzilla_core::error::ParseError) -> JsError {
    JsError::new(&error.to_string())
}
