//! The façade the interface talks to.
//!
//! A [`Session`] owns the ranges, the board, the dead cards and the filters, keeps
//! the per-board classification cache fresh, and hands out everything a panel needs
//! to draw itself. Panels never touch the classifier directly, which is what lets
//! new analysis modes - preflop statistics over all 22,100 flops, the flop
//! breakdown tool, multiway equity - be added here without touching the interface.
//!
//! Players are a `Vec` from the start even though the interface shows two, because
//! multiway is the next thing to arrive.

use std::cell::RefCell;

use crate::board::Board;
use crate::breakdown::{self, Breakdown, BreakdownMode, OverlapMatrix};
use crate::cards::{CardSet, Combo, HandClass, NUM_CLASSES, NUM_COMBOS, SUIT_CHARS};
use crate::equity::{self, ComboEquity, EquityReport, HotCard};
use crate::error::ParseError;
use crate::flops::{self, FlopBreakdown, FlopFilter};
use crate::groups::{Colour, GroupSet, Mark, DEFAULT_COLOUR};
use crate::library::{self, Actions};
use crate::preflop::{self, PreflopBreakdown};
use crate::range::{Preset, Range};
use crate::ranking::Ranking;
use crate::rng::Rng;
use crate::stats::{stat_count, ClassifyOptions, ComboStats, StatBlock, StatId, StatMask};

/// One seat: a range and the filters applied to it.
#[derive(Clone, Debug)]
pub struct Player {
    /// The name shown on the seat's tab.
    pub name: String,
    /// The known hand this seat holds, if it holds one rather than a range.
    ///
    /// A hand is a different kind of participant from a range, not a range that
    /// happens to be small. It has been dealt, so its two cards are out of the
    /// deck for everyone else; there is nothing about it to edit; and it is
    /// made deliberately and removed whole. A range you narrow down to one
    /// combination is still a range - nobody has seen it.
    pub hand: Option<Combo>,
    /// The seat's starting hands.
    pub range: Range,
    /// How the seat's hands are painted. See [`crate::groups`].
    pub groups: GroupSet,
    /// Whether the reader has said anything about how this seat is painted.
    ///
    /// Until they have, the seat follows the default grouping as the board
    /// changes; afterwards it is theirs, and a new card leaves it alone.
    pub painted: bool,
    /// Which streets have had their filter pressed. See [`StreetFilter`].
    pub streets: [Option<StreetFilter>; 3],
    /// The strongest slice of the range, taken by equity. See [`Cut`].
    pub cut: Option<Cut>,
    /// Where the range slider's handles sit. See [`Slider`].
    pub slider: Slider,
    /// The library chart this seat was loaded from, if it was.
    ///
    /// Kept after the range has been edited, rather than forgotten: a reader
    /// who takes the top off a defending chart is still reading that spot, and
    /// the panel says so - the chart is still named, marked as no longer the
    /// chart as it ships.
    pub from_library: Option<FromLibrary>,
}

/// A chart a seat was loaded from, and the range it arrived as.
#[derive(Clone, Debug)]
pub struct FromLibrary {
    /// Which chart, by [`library::Chart::id`].
    pub id: String,
    /// What it put in the matrix, for telling an edit from the chart itself.
    pub pristine: Range,
}

/// One band of the equity range.
///
/// The boundaries are quarters, which is the reading they are meant to carry:
/// better than three hands in four, better than half, worse than half, worse
/// than three in four.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum EquityBand {
    /// Ahead of three quarters of what it is against.
    Best,
    /// Ahead, but not by that much.
    Good,
    /// Behind, but not by that much.
    Weak,
    /// Behind three quarters of it.
    Trash,
}

impl EquityBand {
    /// Every band, strongest first.
    pub const ALL: [EquityBand; 4] = [Self::Best, Self::Good, Self::Weak, Self::Trash];

    /// Which band a hand's equity falls in.
    pub fn of(equity: f32) -> Self {
        if equity >= 0.75 {
            Self::Best
        } else if equity >= 0.50 {
            Self::Good
        } else if equity >= 0.25 {
            Self::Weak
        } else {
            Self::Trash
        }
    }

    /// A stable identifier.
    pub const fn key(self) -> &'static str {
        match self {
            Self::Best => "best",
            Self::Good => "good",
            Self::Weak => "weak",
            Self::Trash => "trash",
        }
    }

    /// The name shown in the panel.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Best => "best hands",
            Self::Good => "good hands",
            Self::Weak => "weak hands",
            Self::Trash => "trash hands",
        }
    }

    /// The bottom of the band, as a percentage.
    pub const fn low(self) -> u8 {
        match self {
            Self::Best => 75,
            Self::Good => 50,
            Self::Weak => 25,
            Self::Trash => 0,
        }
    }

    /// The top of it.
    pub const fn high(self) -> u8 {
        match self {
            Self::Best => 100,
            Self::Good => 75,
            Self::Weak => 50,
            Self::Trash => 25,
        }
    }
}

/// How much of a range sits in one band of equity.
#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct EquityBucket {
    /// A stable identifier.
    pub key: &'static str,
    /// The name shown in the panel.
    pub label: &'static str,
    /// The bottom of the band, as a percentage.
    pub low: u8,
    /// The top of it.
    pub high: u8,
    /// Weighted combos in the band.
    pub combos: f64,
    /// Share of the range, in `0.0..=1.0`.
    pub fraction: f64,
}

/// What the range slider is cutting, and where its two handles sit.
///
/// Both handles are percentages of the whole deck: nought is no hands and a
/// hundred is every hand there is, whatever the matrix holds. That is what lets
/// the slider widen a range as well as narrow it.
///
/// What the matrix changes is the *order* they cut in. A range that arrived
/// some other way - a chart, a painted matrix, a restored link - puts its own
/// cells at the head of the ordering, so it is exactly the band from nought to
/// its own width and the handles park there. See [`Range::window_of`].
///
/// The base is remembered rather than worked out afresh each time, so dragging
/// a handle back brings the chart back whole instead of leaving the reader with
/// whatever their last drag happened to spare.
#[derive(Clone, Debug, Default)]
pub struct Slider {
    /// The range whose ordering the handles cut. Empty means the plain ranking.
    pub base: Range,
    /// Where the band starts, as a percentage of the deck.
    pub low: f64,
    /// Where it ends.
    pub high: f64,
}

/// One combination of one matrix cell, as the cell draws it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct CellCombo {
    /// Which hand.
    pub combo: Combo,
    /// How much of it the range holds.
    pub weight: f32,
    /// The colour it carries. See [`crate::groups`].
    pub colour: Colour,
    /// How much of it survives the street filters.
    pub passing: f32,
    /// Whether a hovered statistic is about this hand.
    pub matches: bool,
    /// Whether the board or the dead cards have already taken one of its cards.
    pub dealt: bool,
}

/// A street's filter: the painted hands, kept once the board moves past it.
///
/// While the board is still at the street it was pressed on, the filter follows
/// the painting - repaint a category and the count moves with it, which is what
/// "the filters are on" means. It freezes the moment a card lands, because from
/// then on the hands that continued are a fact about the past: a flushdraw that
/// bricked is still in the range, even though it is no longer a flushdraw.
#[derive(Clone, PartialEq, Debug)]
pub struct StreetFilter {
    /// The board length it was pressed at, so it knows when it is history.
    pub dealt: usize,
    /// The hands it kept, once it stopped following the painting.
    pub frozen: Option<Range>,
}

/// The strongest slice of a range, taken by equity on one board.
///
/// This is what "continue with the top 20%" means. It cannot be expressed as
/// statistic filters: those name hand classes, and a class is a poor proxy for
/// strength. A nut flushdraw and a four-high flushdraw are one statistic and are
/// nowhere near each other in equity, while a flushdraw and a middle pair are
/// two statistics that often sit side by side. Equity orders them all on the one
/// scale that decides whether continuing is right.
///
/// The slice is frozen as combos rather than kept as a threshold to re-apply.
/// That is both faster - the enumeration runs once - and truer to the game: the
/// hands you continue with on the flop *are* your range on the turn, including
/// the flushdraw that bricked.
#[derive(Clone, Debug, PartialEq)]
pub struct Cut {
    /// The share that was asked for.
    pub share: f64,
    /// The share the slice actually holds, which lands on a combo boundary.
    pub covered: f64,
    /// The equity of the weakest combo that continues.
    pub threshold: f32,
    /// Where the slice starts, as a share of the range. Nought is the top.
    pub from: f64,
    /// The weakest hand in the slice, which is the one a handle is resting on.
    pub hand: Option<String>,
    /// The board the slice was taken on, as text.
    pub board: String,
    /// The painting from before the cut, so dropping it puts that back.
    pub restore: GroupSet,
    /// The combos that continue, at the weight they carry in the range.
    pub range: Range,
}

impl Player {
    /// An empty seat.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            cut: None,
            name: name.into(),
            hand: None,
            range: Range::empty(),
            groups: GroupSet::new(),
            painted: false,
            streets: [None, None, None],
            slider: Slider::default(),
            from_library: None,
        }
    }

    /// A seat that is one known hand.
    pub fn hand(combo: Combo) -> Self {
        let mut range = Range::empty();
        range.set(combo, 1.0);
        Self {
            cut: None,
            name: combo.to_string(),
            hand: Some(combo),
            range,
            groups: GroupSet::new(),
            painted: true,
            streets: [None, None, None],
            slider: Slider::default(),
            from_library: None,
        }
    }

    /// Whether this seat can be edited at all.
    pub fn is_hand(&self) -> bool {
        self.hand.is_some()
    }
}

/// Why the default grouping is being reconsidered.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Because {
    /// A card landed or was taken back.
    TheBoardMoved,
    /// The seat is holding different hands than it was.
    TheRangeChanged,
}

/// Everything the interface needs, in one place.
#[derive(Debug)]
pub struct Session {
    players: Vec<Player>,
    active: usize,
    board: Board,
    dead: CardSet,
    options: ClassifyOptions,
    mode: BreakdownMode,
    ranking: Ranking,
    checkmarks: StatMask,
    /// Which flops a pass over them should look at.
    ///
    /// It belongs to the table rather than to a seat: the point of narrowing to
    /// two-tone ace-high boards is to ask every range the same question, so
    /// moving between seats has to leave the question standing.
    flop_filter: FlopFilter,
    /// The colour the palette is holding.
    colour: Colour,
    cache: ComboStats,
    /// Kept across deals so a run of random flops does not repeat itself.
    rng: Rng,
    /// The last equity calculation, and a fingerprint of what it was of.
    ///
    /// Range against range is a full enumeration - tens of milliseconds - and
    /// the readout is on screen the whole time, so the interface asks for it on
    /// every repaint. Hashing the inputs costs microseconds; doing the work
    /// again costs a visibly laggy interface with two ranges filled in.
    /// Which seat the statistics panel shows a second column for, if any.
    compare: Option<usize>,
    /// Which seat the per-hand equity views measure the active seat against.
    ///
    /// `None` means every other seat at once, taken together. A specific seat
    /// means that seat alone, which is the only way to ask about two of them
    /// when there are three in the pot.
    versus: Option<usize>,
    /// The last pass over all the flops, and what it was a pass over.
    ///
    /// Kept because the pass costs a second and a half and the thing a reader
    /// changes most often afterwards - which statistics count as a hit - does
    /// not change what the pass found, only what is asked of it.
    /// The passes over the flops that have been run, newest last.
    ///
    /// One slot used to be enough, because there was one range. With several
    /// seats the reader runs a pass, switches to the other range to compare,
    /// and switching back used to find the first answer gone - so the work was
    /// done twice to look at the same two numbers. It is keyed by what the pass
    /// was over, so two seats holding the same range share one entry, and it is
    /// capped: a pass is large, and nobody compares seven of them.
    preflop_cache: RefCell<Vec<(u64, PreflopBreakdown)>>,
    /// Per-hand equity before the flop, filed by the pair of ranges it is
    /// between rather than by the seat that asked for it.
    ///
    /// A pass works out both directions, because the graph draws both curves -
    /// and the other seat's question *is* the other direction. Filed by seat it
    /// would be asked again the moment the reader looked from the other side;
    /// filed by pair, switching seats finds the answer already there.
    preflop_equity: RefCell<Vec<(u64, ComboEquity)>>,
    /// What share of each colour a street filter lets through, `0..=1`.
    ///
    /// One per palette colour, unpainted first. Painting says which hands the
    /// filter is about; this says how often they actually continue, which is
    /// how a mixed strategy is written down.
    colour_shares: [f32; crate::groups::COLOURS + 1],
    equity_cache: RefCell<EquityCache>,
}

/// One cached calculation, and a fingerprint of what it was of.
#[derive(Debug)]
struct Cached<T> {
    key: u64,
    value: T,
}

/// The equity calculations, each remembered against its own inputs.
///
/// Separate keys rather than one for the lot, because they do not read the same
/// things: the readout compares the ranges as written, while the graph compares
/// what is left after the filters. Sharing a key would recompute the readout
/// every time a category was painted, which is the hot path.
#[derive(Default, Debug)]
struct EquityCache {
    report: Option<Cached<Option<EquityReport>>>,
    by_combo: Option<Cached<Option<ComboEquity>>>,
    opponent: Option<Cached<Option<ComboEquity>>>,
}

impl Session {
    /// How many seats a session will hold.
    ///
    /// Six, because that is a six-handed table and because every extra seat
    /// makes the multiway sampling slower: a deal where two players need the
    /// same card is thrown away, and with six wide ranges that happens often.
    pub const MAX_SEATS: usize = 6;

    /// A session with two empty seats and no board.
    pub fn new() -> Self {
        let board = Board::empty();
        let options = ClassifyOptions::default();
        Self {
            players: vec![Player::new("Range A"), Player::new("Range B")],
            active: 0,
            rng: Rng::new(equity::MONTE_CARLO_SEED),
            compare: None,
            versus: None,
            preflop_cache: RefCell::new(Vec::new()),
            preflop_equity: RefCell::new(Vec::new()),
            colour_shares: [1.0; crate::groups::COLOURS + 1],
            equity_cache: RefCell::new(EquityCache::default()),
            board,
            dead: CardSet::EMPTY,
            options,
            mode: BreakdownMode::Absolute,
            ranking: Ranking::default(),
            checkmarks: StatMask::EMPTY,
            flop_filter: FlopFilter::EVERYTHING,
            colour: DEFAULT_COLOUR,
            cache: ComboStats::build(&board, CardSet::EMPTY, options),
        }
    }

    fn rebuild(&mut self) {
        self.cache = ComboStats::build(&self.board, self.dead, self.options);
        self.apply_default_groups(Because::TheBoardMoved);
    }

    /// Paints any seat nobody has taken over with the default grouping.
    ///
    /// Flopzilla opens on top pair or better, the flushdraws and the open-enders,
    /// and so does this - there is nothing to say about a grouping before a flop,
    /// so this runs whenever the board or a range moves under it. Painting
    /// anything at all, including painting nothing, makes it the reader's, and
    /// from then on a new card leaves it alone.
    fn apply_default_groups(&mut self, because: Because) {
        if !self.board.is_dealt() {
            return;
        }
        for index in 0..self.players.len() {
            let empty = self.players[index].groups.is_empty();
            // A card landing leaves a cleared grouping cleared: the reader said
            // to unpaint it and nothing has happened since to change the
            // question. A new range is a new question, so an empty grouping gets
            // the default back rather than leaving the reader with a range the
            // panel has no opinion about.
            let wanted = match because {
                Because::TheBoardMoved => !self.players[index].painted,
                Because::TheRangeChanged => empty,
            };
            if wanted {
                self.players[index].groups = GroupSet::default_for(&self.cache);
                self.players[index].painted = false;
            }
        }
    }

    // ----- board and dead cards -------------------------------------------------

    /// The community cards.
    pub fn board(&self) -> &Board {
        &self.board
    }

    /// Replaces the board.
    pub fn set_board(&mut self, board: Board) {
        if board.len() != self.board.len() {
            // Keep what each street's filter holds before the board moves on:
            // afterwards its categories mean something else.
            self.freeze_streets(board.len());
        }
        self.board = board;
        self.dead = self.dead.difference(board.mask());
        self.rebuild();
    }

    /// Replaces the board from text such as `Kc Qh Jh`.
    pub fn set_board_text(&mut self, text: &str) -> Result<(), ParseError> {
        self.set_board(Board::parse(text)?);
        Ok(())
    }

    /// Adds one card to the board, if there is room and it is not already dealt.
    pub fn push_board_card(&mut self, card: crate::cards::Card) -> bool {
        if self.dead.contains(card) {
            return false;
        }
        match self.board.with_card(card) {
            Some(board) => {
                self.set_board(board);
                true
            }
            None => false,
        }
    }

    /// Removes the last board card.
    pub fn pop_board_card(&mut self) {
        let board = self.board.without_last();
        self.set_board(board);
    }

    /// Shows the board up to `len` cards, the way the street arrows do.
    pub fn truncate_board(&mut self, len: usize) {
        let board = self.board.truncated(len);
        self.set_board(board);
    }

    /// The dead cards.
    pub fn dead(&self) -> CardSet {
        self.dead
    }

    /// Replaces the dead cards, dropping anything already on the board.
    pub fn set_dead(&mut self, dead: CardSet) {
        self.dead = dead.difference(self.board.mask());
        self.rebuild();
    }

    /// Adds or removes one dead card.
    pub fn toggle_dead(&mut self, card: crate::cards::Card) {
        if self.board.mask().contains(card) {
            return;
        }
        let mut dead = self.dead;
        if dead.contains(card) {
            dead.remove(card);
        } else {
            dead.insert(card);
        }
        self.set_dead(dead);
    }

    // ----- seats ----------------------------------------------------------------

    /// Every seat.
    pub fn players(&self) -> &[Player] {
        &self.players
    }

    /// The seat currently shown in the matrix and the statistics panel.
    pub fn active_index(&self) -> usize {
        self.active
    }

    /// Selects a seat.
    pub fn set_active(&mut self, index: usize) {
        if index < self.players.len() {
            self.active = index;
        }
    }

    /// Adds a seat. Multiway analysis will read these.
    pub fn add_player(&mut self, name: impl Into<String>) -> usize {
        self.players.push(Player::new(name));
        self.players.len() - 1
    }

    /// Adds a seat, named after the letter it lands on.
    ///
    /// Returns `None` once the table is full. Six is where a hold'em table
    /// stops being a table anyone plays, and the sampling gets slower with each
    /// seat because a deal that needs the same card twice has to be thrown away.
    pub fn add_seat(&mut self) -> Option<usize> {
        if self.players.len() >= Self::MAX_SEATS {
            return None;
        }
        let ranges = self.players.iter().filter(|p| !p.is_hand()).count();
        let letter = char::from(b'A' + ranges as u8);
        Some(self.add_player(format!("Range {letter}")))
    }

    /// Adds a seat holding a copy of one that is already there.
    ///
    /// The matrix, the painting and whatever the street filters are holding,
    /// so the copy is somewhere to try a change without losing what it was a
    /// change from - which is the whole point of having it.
    ///
    /// A dealt hand cannot be copied: its two cards are out of the deck, and a
    /// second seat holding them would be the same cards dealt twice.
    pub fn duplicate_seat(&mut self, index: usize) -> Option<usize> {
        if self.players.len() >= Self::MAX_SEATS {
            return None;
        }
        let player = self.players.get(index)?;
        if player.is_hand() {
            return None;
        }
        let mut copy = player.clone();
        let ranges = self.players.iter().filter(|p| !p.is_hand()).count();
        copy.name = format!("Range {}", char::from(b'A' + ranges as u8));
        self.players.push(copy);
        Some(self.players.len() - 1)
    }

    /// Removes a seat, keeping at least two.
    ///
    /// Returns whether it went. The seats after it move up, and the names move
    /// with them so the letters stay in order - a table with a gap at B reads
    /// as a bug rather than as a choice.
    pub fn remove_seat(&mut self, index: usize) -> bool {
        let Some(player) = self.players.get(index) else {
            return false;
        };
        let ranges = self.players.iter().filter(|p| !p.is_hand()).count();
        if !player.is_hand() && ranges <= 2 {
            return false;
        }
        self.players.remove(index);
        // The letters close the gap: a table with a hole at B reads as a bug
        // rather than as a choice. A hand is named after its cards and has no
        // letter to lose.
        let mut letter = b'A';
        for player in self.players.iter_mut() {
            if player.is_hand() {
                continue;
            }
            player.name = format!("Range {}", char::from(letter));
            letter += 1;
        }
        self.active = self.active.min(self.players.len() - 1);
        true
    }

    /// Whether the active seat is something the reader can change.
    ///
    /// A hand has been dealt. There is nothing to paint on it, nothing to
    /// filter out of it and nothing to widen: it is two cards, and the only
    /// things you can do with it are make it and take it away. Saying so in one
    /// place keeps every mutator from having to remember.
    pub fn editable(&self) -> bool {
        !self.active().is_hand()
    }

    /// Adds a seat holding one known hand, leaving the selection where it is.
    ///
    /// The only way to make one: clicking cells in the matrix builds a range,
    /// however few hands end up in it, because nobody has seen those cards.
    /// Returns `None` when the table is full, or when the hand needs a card
    /// that is already on the board, in the dead cards, or in another hand.
    ///
    /// Selecting it would be the obvious thing and is the wrong thing: there is
    /// nothing to do to a hand, so the reader would arrive at a panel where
    /// every control is dead and have to click their way back to the range they
    /// were working on. Dealing a hand is something you do *to* the table while
    /// working on a range, so the range keeps the floor.
    pub fn add_hand(&mut self, hand: Combo) -> Option<usize> {
        if self.players.len() >= Self::MAX_SEATS {
            return None;
        }
        let taken = self.board.mask().union(self.dead).union(self.dealt_hands());
        if hand.mask().intersects(taken) {
            return None;
        }
        self.players.push(Player::hand(hand));
        Some(self.players.len() - 1)
    }

    /// The seats taking part in the equity readout, in seat order.
    ///
    /// A seat with nothing left in it after its filters is not in the pot: it
    /// has no hands to be dealt. So the readout is about the seats that do, and
    /// this says which they are, because the answer carries no names.
    pub fn equity_seats(&self) -> Vec<usize> {
        (0..self.players.len())
            .filter(|index| !self.narrowed_for(*index).is_empty())
            .collect()
    }

    /// A seat by index.
    pub fn player(&self, index: usize) -> Option<&Player> {
        self.players.get(index)
    }

    /// The active seat.
    pub fn active(&self) -> &Player {
        &self.players[self.active]
    }

    /// The active seat, mutably.
    pub fn active_mut(&mut self) -> &mut Player {
        &mut self.players[self.active]
    }

    // ----- range editing --------------------------------------------------------

    /// The ordering used by the range slider.
    pub fn ranking(&self) -> Ranking {
        self.ranking
    }

    /// Chooses the ordering used by the range slider.
    pub fn set_ranking(&mut self, ranking: Ranking) {
        self.ranking = ranking;
        // Where a handle sits is a statement about an ordering, so it has to be
        // made again about the new one.
        let was = self.active;
        for seat in 0..self.players.len() {
            self.active = seat;
            self.park_slider();
        }
        self.active = was;
    }

    /// Replaces the active range with the top `percent` of hands.
    pub fn set_active_top_percent(&mut self, percent: f64) {
        if !self.editable() {
            return;
        }
        let ranking = self.ranking;
        self.active_mut().range = Range::top_percent(percent, ranking);
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
    }

    /// Replaces the active range with the band between the slider handles.
    pub fn set_active_window(&mut self, from: f64, to: f64) {
        if !self.editable() {
            return;
        }
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        let ranking = self.ranking;
        let base = self.active().slider.base.clone();
        self.active_mut().slider.low = low;
        self.active_mut().slider.high = high;
        self.active_mut().range = base.window_of(low, high, ranking);
        self.apply_default_groups(Because::TheRangeChanged);
    }

    /// What the slider is cutting, and where its handles sit.
    pub fn slider(&self) -> &Slider {
        &self.active().slider
    }

    /// Points the slider at what the matrix now holds, handles parked on it.
    ///
    /// Called by every way of setting a range except the slider itself: the one
    /// range the handles must not be re-pointed at is the one they just made,
    /// or dragging one back would never return what it took away.
    fn park_slider(&mut self) {
        let range = self.active().range.clone();
        // Exactly the width, not rounded to anything: it is a stop by
        // construction - the edge of the last cell the range holds - and
        // rounding would move the handle off the one place where it selects
        // what the matrix already has.
        let high = range.cell_percent();
        self.active_mut().slider = Slider {
            base: range,
            low: 0.0,
            high,
        };
    }

    /// The range split by how much equity each hand has.
    ///
    /// A different question from the ladder above it, and a better one for
    /// some purposes: the ladder says what a hand *is*, and this says what it
    /// is worth. Second pair with a flushdraw and top pair with nothing are
    /// rungs apart and about the same hand to play.
    ///
    /// Four bands rather than ten, because the reader is being asked to take
    /// in a shape rather than look a number up - there is an equity graph two
    /// panels over for that.
    ///
    /// Empty when there is no per-hand equity to be had: nobody to measure
    /// against, or no board and no pass over the flops yet.
    pub fn equity_buckets(&self) -> Vec<EquityBucket> {
        let Some(equity) = self.equity_by_combo() else {
            return Vec::new();
        };
        let mine = self.effective_range();
        let mut held = [0.0f64; EquityBand::ALL.len()];
        let mut total = 0.0f64;
        for combo in self.cache.live().iter() {
            let weight = f64::from(mine.get(combo));
            let value = equity.equity[combo.index() as usize];
            if weight <= 0.0 || value < 0.0 {
                continue;
            }
            held[EquityBand::of(value) as usize] += weight;
            total += weight;
        }
        if total <= 0.0 {
            return Vec::new();
        }
        EquityBand::ALL
            .into_iter()
            .map(|band| EquityBucket {
                key: band.key(),
                label: band.label(),
                low: band.low(),
                high: band.high(),
                combos: held[band as usize],
                fraction: held[band as usize] / total,
            })
            .collect()
    }

    /// Where the slider's handles have somewhere to stop.
    ///
    /// The edges of the cells, in the order the slider walks them, as
    /// percentages. Between two of them there is nothing to choose, so this is
    /// what the handles snap to.
    pub fn slider_stops(&self) -> Vec<f32> {
        self.active().slider.base.slider_stops(self.ranking)
    }

    /// Removes the top `percent` of hands from the active range.
    pub fn remove_active_top_percent(&mut self, percent: f64) {
        let ranking = self.ranking;
        let range = self.active().range.without_top_percent(percent, ranking);
        self.active_mut().range = range;
        self.park_slider();
    }

    /// Replaces the active range from text.
    pub fn set_active_range_text(&mut self, text: &str) -> Result<(), ParseError> {
        if !self.editable() {
            return Ok(());
        }
        let range = Range::parse(text)?;
        self.active_mut().range = range;
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
        Ok(())
    }

    /// Sets one matrix cell on the active range.
    pub fn set_active_class(&mut self, class: HandClass, weight: f32) {
        self.active_mut().range.set_class(class, weight);
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
    }

    /// The individual combos of one matrix cell, with their weights.
    ///
    /// This is what a suit-level editor needs: the model has always been
    /// combo-level, so picking `AhKh` out of `AKs` costs nothing extra.
    pub fn class_combos(&self, class: HandClass) -> Vec<(Combo, f32)> {
        let range = &self.active().range;
        class
            .combos()
            .map(|combo| (combo, range.get(combo)))
            .collect()
    }

    /// Everything one matrix cell holds, combination by combination.
    ///
    /// The cell is a summary of four, six or twelve hands, and every question
    /// the matrix answers for a cell has an answer for each of them: how much
    /// of it is in the range, what colour it carries, whether it survived the
    /// filters, and whether it is one of the hands a hovered statistic is
    /// about. One reader for all of it, because they are drawn together.
    pub fn cell_combos(&self, class: HandClass, within: Option<StatId>) -> Vec<CellCombo> {
        let range = &self.active().range;
        let narrowed = self.narrowed();
        let blocked = self.dealt_elsewhere(self.active);
        class
            .combos()
            .map(|combo| CellCombo {
                combo,
                weight: range.get(combo),
                colour: self.active().groups.get(combo, &self.cache),
                passing: narrowed.get(combo),
                matches: within.is_some_and(|stat| self.cache.mask(combo).has(stat)),
                dealt: combo.mask().intersects(blocked),
            })
            .collect()
    }

    /// Sets the weight of one specific combo on the active range.
    pub fn set_combo_weight(&mut self, combo: Combo, weight: f32) {
        if !self.editable() {
            return;
        }
        self.active_mut().range.set(combo, weight);
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
    }

    /// Replaces the active range with a chart from the library.
    ///
    /// Returns `false` if there is no such chart, so an old saved link naming one
    /// that has since been renamed fails quietly rather than throwing.
    pub fn load_library(&mut self, id: &str) -> bool {
        self.load_library_chart(id, Actions::ALL, false)
    }

    /// Loads a chart, optionally without the hands whose EV is zero.
    pub fn load_library_chart(
        &mut self,
        id: &str,
        actions: Actions,
        without_zero_ev: bool,
    ) -> bool {
        if !self.editable() {
            return false;
        }
        let built = |chart: &library::Chart| {
            if without_zero_ev {
                chart.range_of_without_zero_ev(actions).ok()
            } else {
                chart.range_of(actions).ok()
            }
        };
        match library::chart_by_id(id).and_then(built) {
            Some(range) => {
                self.active_mut().from_library = Some(FromLibrary {
                    id: id.to_string(),
                    pristine: range.clone(),
                });
                self.active_mut().range = range;
                self.park_slider();
                self.apply_default_groups(Because::TheRangeChanged);
                true
            }
            None => false,
        }
    }

    /// Which library chart the active seat was loaded from.
    pub fn from_library(&self) -> Option<&FromLibrary> {
        self.active().from_library.as_ref()
    }

    /// Whether the active range is no longer the chart it was loaded from.
    ///
    /// A cell painted, a handle dragged, a line typed - anything that leaves
    /// the matrix holding something other than what the chart shipped. The
    /// panel says so rather than letting go of the chart: the reader is still
    /// looking at that spot, just not at the solver's answer to it.
    pub fn library_edited(&self) -> bool {
        self.active()
            .from_library
            .as_ref()
            .is_some_and(|chart| chart.pristine != self.active().range)
    }

    /// Loads a chart with another one taken out of it.
    ///
    /// Weight by weight rather than hand by hand, so a chart that three-bets
    /// ace-king a third of the time takes a third of it away.
    ///
    /// Nothing in the interface reaches this. It was how a cold call was got at
    /// before the charts carried their actions apart; now the call is one of
    /// them and is asked for directly. Kept because taking one named range off
    /// another is a thing a reader may yet want to do.
    ///
    /// The seat is left holding `id` rather than an edit of it.
    pub fn load_library_less(&mut self, id: &str, minus: &str, without_zero_ev: bool) -> bool {
        if !self.editable() {
            return false;
        }
        let built = |id: &str| {
            library::chart_by_id(id).and_then(|chart| {
                if without_zero_ev {
                    chart.range_without_zero_ev().ok()
                } else {
                    chart.range().ok()
                }
            })
        };
        let (Some(whole), Some(taken)) = (built(id), built(minus)) else {
            return false;
        };
        let left = whole.difference(&taken);
        self.active_mut().from_library = Some(FromLibrary {
            id: id.to_string(),
            pristine: left.clone(),
        });
        self.active_mut().range = left;
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
        true
    }

    /// Takes a chart away from the active range.
    ///
    /// Nothing in the interface reaches this either: a chart carries its
    /// actions apart, so the parts of one are asked for rather than worked out.
    /// Kept for the same reason as [`Session::load_library_less`].
    ///
    /// Returns `false` if there is no such chart, like [`Session::load_library`].
    pub fn subtract_library_chart(&mut self, id: &str, without_zero_ev: bool) -> bool {
        if !self.editable() {
            return false;
        }
        let Some(other) = library::chart_by_id(id).and_then(|chart| {
            if without_zero_ev {
                chart.range_without_zero_ev().ok()
            } else {
                chart.range().ok()
            }
        }) else {
            return false;
        };
        self.subtract(&other);
        true
    }

    /// Takes everything a quick button selects away from the active range.
    ///
    /// Nothing in the interface reaches this either; see
    /// [`Session::subtract_library_chart`].
    pub fn subtract_preset(&mut self, preset: Preset) {
        if !self.editable() {
            return;
        }
        self.subtract(&Range::preset(preset));
    }

    fn subtract(&mut self, other: &Range) {
        let left = self.active().range.difference(other);
        self.active_mut().range = left;
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
    }

    /// Adds everything a quick button selects to the active range.
    pub fn add_preset(&mut self, preset: Preset) {
        let merged = self.active().range.union(&Range::preset(preset));
        self.active_mut().range = merged;
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
    }

    /// Empties the active range.
    pub fn clear_active_range(&mut self) {
        if !self.editable() {
            return;
        }
        self.active_mut().range = Range::empty();
        // Clearing is starting again rather than editing, so the chart the seat
        // came from goes with it.
        self.active_mut().from_library = None;
        self.park_slider();
        self.apply_default_groups(Because::TheRangeChanged);
    }

    // ----- options --------------------------------------------------------------

    /// The classifier settings.
    pub fn options(&self) -> ClassifyOptions {
        self.options
    }

    /// Replaces the classifier settings and reclassifies.
    pub fn set_options(&mut self, options: ClassifyOptions) {
        self.options = options;
        self.rebuild();
    }

    /// Whether rows report their own share or their share plus everything above.
    pub fn mode(&self) -> BreakdownMode {
        self.mode
    }

    /// Chooses absolute or cumulative reporting.
    pub fn set_mode(&mut self, mode: BreakdownMode) {
        self.mode = mode;
    }

    /// The colour the palette is holding.
    pub fn colour(&self) -> Colour {
        self.colour
    }

    /// Picks the colour to paint with.
    pub fn set_colour(&mut self, colour: Colour) {
        self.colour = colour;
    }

    /// Paints every hand in the range that carries a statistic.
    pub fn paint_stat(&mut self, stat: StatId, colour: Colour) {
        if !self.editable() {
            return;
        }
        self.active_mut().painted = true;
        let mut groups = std::mem::take(&mut self.active_mut().groups);
        groups.paint_stat(stat, colour, &self.cache);
        self.active_mut().groups = groups;
    }

    /// Paints part of a category, taken by equity.
    ///
    /// `from` and `to` are percentages of the category, strongest first: the
    /// whole of it is `0..100`, its best fifth is `0..20`, its worst quarter
    /// is `75..100`. A reader building a betting range wants the best of the
    /// rubbish in it and the worst of the good, and the category on its own
    /// cannot say which those are - two hands on one rung can be a long way
    /// apart in what they are worth.
    ///
    /// The whole of a category is painted as a category, so it keeps following
    /// the board as cards come. A part of one cannot: which hands are in it is
    /// a fact about this board, so those are painted hand by hand and stay
    /// where they were put.
    ///
    /// Returns whether there was anything to paint.
    pub fn paint_stat_part(&mut self, stat: StatId, from: f64, to: f64, colour: Colour) -> bool {
        if from <= 0.0 && to >= 100.0 {
            self.paint_stat(stat, colour);
            return true;
        }
        let held = self.hands_carrying(|mask| mask.has(stat));
        self.paint_slice(&held, from, to, colour)
    }

    /// Paints part of one band of the equity range, taken by equity.
    ///
    /// The band says which hands; `from` and `to` say which part of them, in
    /// the same way as [`Session::paint_stat_part`].
    pub fn paint_equity_band(
        &mut self,
        band: EquityBand,
        from: f64,
        to: f64,
        colour: Colour,
    ) -> bool {
        let Some(equity) = self.equity_by_combo() else {
            return false;
        };
        let mine = self.effective_range();
        let held: Vec<(Combo, f32, f32)> = self
            .cache
            .live()
            .iter()
            .filter_map(|combo| {
                let weight = mine.get(combo);
                let value = equity.equity[combo.index() as usize];
                let wanted = weight > 0.0 && value >= 0.0 && EquityBand::of(value) == band;
                wanted.then_some((combo, weight, value))
            })
            .collect();
        self.paint_slice(&held, from, to, colour)
    }

    /// Every hand in the range whose classification answers `wanted`, with what
    /// it weighs and what it is worth.
    fn hands_carrying(&self, wanted: impl Fn(StatMask) -> bool) -> Vec<(Combo, f32, f32)> {
        let equity = self.equity_by_combo();
        let mine = self.effective_range();
        self.cache
            .live()
            .iter()
            .filter_map(|combo| {
                let weight = mine.get(combo);
                if weight <= 0.0 || !wanted(self.cache.mask(combo)) {
                    return None;
                }
                let value = equity
                    .as_ref()
                    .map_or(-1.0, |by| by.equity[combo.index() as usize]);
                Some((combo, weight, value))
            })
            .collect()
    }

    /// Paints the `from..to` percent of some hands, strongest first.
    ///
    /// Whole runs of equal equity move together, as everywhere else a slice is
    /// taken by equity: three of a pair of aces is not a decision anybody
    /// makes, it is an artefact of where a boundary happened to land.
    fn paint_slice(
        &mut self,
        held: &[(Combo, f32, f32)],
        from: f64,
        to: f64,
        colour: Colour,
    ) -> bool {
        if !self.editable() || held.is_empty() {
            return false;
        }
        // With no equity to sort by there is no part to take, so the whole of
        // it is painted rather than an arbitrary piece.
        let sortable = held.iter().all(|(_, _, value)| *value >= 0.0);
        let mut order: Vec<(Combo, f32, f32)> = held.to_vec();
        if sortable {
            order.sort_by(|a, b| b.2.total_cmp(&a.2));
        }
        let total: f64 = order.iter().map(|(_, weight, _)| f64::from(*weight)).sum();
        if total <= 0.0 {
            return false;
        }
        let (low, high) = if from <= to { (from, to) } else { (to, from) };
        let start = total * (low / 100.0);
        let end = total * (high / 100.0);

        let mut painted = false;
        let mut covered = 0.0f64;
        let mut at = 0usize;
        while at < order.len() {
            // The run of hands worth exactly the same, taken or left together.
            let mut run = at + 1;
            if sortable {
                while run < order.len() && order[run].2 == order[at].2 {
                    run += 1;
                }
            } else {
                run = order.len();
            }
            let weight: f64 = order[at..run]
                .iter()
                .map(|(_, weight, _)| f64::from(*weight))
                .sum();
            let middle = covered + weight / 2.0;
            if middle >= start - 1e-9 && middle <= end + 1e-9 {
                for (combo, _, _) in &order[at..run] {
                    self.paint_combo(*combo, colour);
                    painted = true;
                }
            }
            covered += weight;
            at = run;
        }
        painted
    }

    /// Paints one hand.
    pub fn paint_combo(&mut self, combo: Combo, colour: Colour) {
        if !self.editable() {
            return;
        }
        self.active_mut().painted = true;
        let mut groups = std::mem::take(&mut self.active_mut().groups);
        groups.set(combo, colour, &self.cache);
        self.active_mut().groups = groups;
    }

    /// The colour of one hand.
    pub fn combo_colour(&self, combo: Combo) -> Colour {
        self.active().groups.get(combo, &self.cache)
    }

    /// How strong every hand is on this board, by combo index.
    ///
    /// The packed rank of the best five cards it makes, and nought where it
    /// makes none - before the flop, or where the board is holding one of its
    /// cards. Two hands can be worth the same equity and not be the same hand:
    /// against one opponent a queen-high flush and a jack-high one win the
    /// same, and it is still the queen-high flush that is the better hand.
    pub fn rank_by_combo(&self) -> Vec<u32> {
        let board = self.board.mask();
        if self.board.cards().count() < 3 {
            return Vec::new();
        }
        (0..crate::cards::NUM_COMBOS)
            .map(|index| {
                let combo = Combo::from_index(index as u16);
                if combo.mask().intersects(board) {
                    return 0;
                }
                crate::eval::eval(board.union(combo.mask())).value()
            })
            .collect()
    }

    /// The colour of every hand, by combo index.
    ///
    /// The pie reads all of them at once, and asking one at a time across the
    /// boundary is the sort of thing that turns a redraw into a stutter.
    pub fn colour_by_combo(&self) -> Vec<Colour> {
        let groups = &self.active().groups;
        (0..crate::cards::NUM_COMBOS)
            .map(|index| groups.get(Combo::from_index(index as u16), &self.cache))
            .collect()
    }

    /// What a statistic's marker shows: one colour, a gear, or nothing.
    pub fn mark(&self, stat: StatId) -> Mark {
        self.active()
            .groups
            .mark(stat, &self.active().range, &self.cache)
    }

    /// How many colours the active range actually uses.
    pub fn colours_used(&self) -> usize {
        self.active()
            .groups
            .colours_used(&self.active().range, &self.cache)
    }

    /// The weight of the range in each colour, unpainted first.
    ///
    /// Of what is left after the street filters, not of the range as written:
    /// once a filter has removed the unpainted hands the pie is all colour, and
    /// a slice for hands that are no longer in the range would be a slice of
    /// nothing. Flopzilla draws it the same way.
    pub fn group_shares(&self) -> [f64; crate::groups::COLOURS + 1] {
        self.active().groups.shares(&self.narrowed(), &self.cache)
    }

    /// Paints every hand that carries both statistics at once.
    ///
    /// Flopzilla puts a key on this - the overlap between two rows is where the
    /// interesting hands are, and reaching them by hand means finding every
    /// flushdraw that is also a pair and clicking each one.
    pub fn paint_intersection(&mut self, a: StatId, b: StatId, colour: Colour) {
        if !self.editable() {
            return;
        }
        self.active_mut().painted = true;
        let wanted = a.mask().union(b.mask());
        let range = self.active().range.clone();
        let mut groups = std::mem::take(&mut self.active_mut().groups);
        for combo in self.cache.live().iter() {
            if range.get(combo) > 0.0 && self.cache.mask(combo).intersection(wanted) == wanted {
                groups.set(combo, colour, &self.cache);
            }
        }
        self.active_mut().groups = groups;
    }

    /// The statistics panel as text, for pasting somewhere else.
    ///
    /// The numbers on screen are the answer to whatever was just asked, and the
    /// usual next step is to put them in a post or a spreadsheet. Restricted to
    /// one statistic when `within` names one, which is what the panel shows
    /// while a row is hovered.
    pub fn statistics_text(&self, within: Option<StatId>) -> String {
        let panel = match within {
            Some(stat) => self.breakdown_within(stat.mask()),
            None => self.breakdown(),
        };
        let mut out = String::new();
        out.push_str(&format!(
            "{}\t{:.1} combos\n",
            if self.board.is_empty() {
                "preflop".to_owned()
            } else {
                self.board.to_string()
            },
            panel.total_combos
        ));
        for row in &panel.rows {
            out.push_str(&format!(
                "{}\t{:.1}\t{:.2}%\n",
                row.label,
                row.combos,
                row.fraction * 100.0
            ));
        }
        out
    }

    /// Every combination the statistics panel is currently speaking about.
    ///
    /// Not the whole range: what is on screen, which is the range after its
    /// street filters and, while a row is hovered, only the hands on that row.
    pub fn statistics_combos(&self, within: Option<StatId>) -> String {
        let range = self.effective_range();
        let mut combos = Vec::new();
        for combo in self.cache.live().iter() {
            if range.get(combo) <= 0.0 {
                continue;
            }
            if let Some(stat) = within {
                if !self.cache.mask(combo).has(stat) {
                    continue;
                }
            }
            combos.push(combo.to_string());
        }
        combos.join(",")
    }

    /// How much of each matrix cell sits in each colour, as a share of the cell.
    ///
    /// A share of the cell's own weight, so a cell that is entirely one colour
    /// reads as entirely that colour however many combinations it holds.
    pub fn class_colours(&self) -> [[f32; crate::groups::COLOURS + 1]; NUM_CLASSES] {
        let mut shares = [[0.0f32; crate::groups::COLOURS + 1]; NUM_CLASSES];
        let mut held = [0.0f32; NUM_CLASSES];
        let player = self.active();
        for combo in self.cache.live().iter() {
            let weight = player.range.get(combo);
            if weight <= 0.0 {
                continue;
            }
            let class = combo.class().index() as usize;
            held[class] += weight;
            shares[class][player.groups.get(combo, &self.cache) as usize] += weight;
        }
        for (cell, total) in shares.iter_mut().zip(held) {
            if total > 0.0 {
                for share in cell.iter_mut() {
                    *share /= total;
                }
            }
        }
        shares
    }

    /// How much of each matrix cell survives the applied street filters.
    ///
    /// This is what greys a cell out: nothing to do with colour, only with
    /// whether the hands are still in the range once the filters have run.
    pub fn class_passing(&self) -> [f32; NUM_CLASSES] {
        let narrowed = self.narrowed();
        let mut passing = [0.0f32; NUM_CLASSES];
        let mut held = [0.0f32; NUM_CLASSES];
        for combo in self.cache.live().iter() {
            let weight = self.active().range.get(combo);
            if weight <= 0.0 {
                continue;
            }
            let class = combo.class().index() as usize;
            held[class] += weight;
            passing[class] += narrowed.get(combo);
        }
        for (share, total) in passing.iter_mut().zip(held) {
            *share = if total > 0.0 { *share / total } else { 0.0 };
        }
        passing
    }

    /// Which suits each suited cell is actually holding, when not all of them.
    ///
    /// A suited hand is the one cell where a suit is unambiguous: each of its
    /// four combinations is one suit, so naming the suits names the hands. A
    /// range that keeps the two hearts of `T9s` and nothing else is a different
    /// range from one that keeps all four, and the cell should say so.
    ///
    /// Nothing is drawn when the cell holds every combination still available,
    /// because then the suits carry no information - and availability is after
    /// card removal, so a cell whose fourth suit is on the board reads as whole
    /// with three.
    ///
    /// The range here is the one on the table: what the filters have left, so
    /// the pips follow a filter being applied as readily as a hand being
    /// painted in by hand.
    pub fn class_suits(&self) -> [Vec<u8>; NUM_CLASSES] {
        let narrowed = self.narrowed();
        let blocked = self.dealt_elsewhere(self.active);
        std::array::from_fn(|index| {
            let class = HandClass::from_index(index as u8);
            if !class.is_suited() {
                return Vec::new();
            }
            let mut held = Vec::new();
            let mut available = 0usize;
            for combo in class.combos() {
                if combo.mask().intersects(blocked) {
                    continue;
                }
                available += 1;
                if narrowed.get(combo) > 0.0 {
                    held.push(combo.cards().0.suit());
                }
            }
            if held.is_empty() || held.len() == available {
                return Vec::new();
            }
            // Strongest suit first, the order the card grids are laid out in,
            // so the pips read the same way everywhere.
            held.sort_unstable_by(|a, b| b.cmp(a));
            held.iter().map(|&suit| SUIT_CHARS[suit as usize]).collect()
        })
    }

    /// Unpaints every hand, leaving the street filters alone.
    pub fn clear_groups(&mut self) {
        if !self.editable() {
            return;
        }
        self.active_mut().groups.clear();
        self.active_mut().painted = true;
        self.active_mut().cut = None;
    }

    /// Swaps painted for unpainted, painting with the held colour.
    /// Swaps which categories are painted, leaving the hands where they fall.
    ///
    /// The marks down the side of the panel flip; the hands follow the
    /// categories rather than being picked one at a time, so nothing grows a
    /// gear and the two halves overlap wherever the categories do.
    pub fn invert_categories(&mut self) {
        if !self.editable() {
            return;
        }
        self.active_mut().painted = true;
        let colour = self.colour;
        self.active_mut().groups.invert_categories(colour);
    }

    /// Swaps painted for unpainted, hand by hand.
    ///
    /// What comes out is exactly what was not in, so the two halves add up to
    /// the range. Where that splits a category - and overlapping categories do
    /// get split - the gear on it is the truth rather than an artefact. See
    /// [`Self::invert_categories`] for the other half of the idea.
    pub fn invert_groups(&mut self) {
        if !self.editable() {
            return;
        }
        self.active_mut().painted = true;
        // Hand by hand, so what comes out is exactly what was not in before and
        // the two halves add up to the range. Where that splits a category -
        // and overlapping categories do get split - the gear on it is the
        // truth, not an artefact.
        let colour = self.colour;
        let mut groups = std::mem::take(&mut self.active_mut().groups);
        groups.invert(colour, &self.cache);
        self.active_mut().groups = groups;
    }

    /// Puts the default grouping back: top pair or better, flushdraws, open-enders.
    pub fn reset_groups(&mut self) {
        if !self.editable() {
            return;
        }
        self.active_mut().groups = GroupSet::default_for(&self.cache);
        self.active_mut().painted = false;
        self.active_mut().cut = None;
    }

    /// Freezes the painted range as the filter for one street.
    ///
    /// `street` is 0 for the flop, 1 for the turn, 2 for the river. Pressing it
    /// again lifts the filter, which is why it is a toggle rather than a button:
    /// the narrowing is a hypothesis, and you want to be able to take it back.
    pub fn toggle_street_filter(&mut self, street: usize) -> bool {
        if !self.editable() {
            return false;
        }
        if street >= 3 {
            return false;
        }
        if self.active().streets[street].is_some() {
            self.active_mut().streets[street] = None;
            return false;
        }
        let dealt = self.board.len();
        self.active_mut().streets[street] = Some(StreetFilter {
            dealt,
            frozen: None,
        });
        true
    }

    /// How many combos would survive if a street's filter were pressed now.
    ///
    /// Flopzilla shows the number whether the filter is on or not, which is the
    /// useful way round: the question is usually "how much would this leave".
    pub fn painted_combos(&self) -> f64 {
        self.active()
            .groups
            .painted_at_weights(&self.active().range, &self.cache, &self.colour_shares)
            .combo_count_excluding(self.dealt_elsewhere(self.active))
    }

    /// What share of a colour a street filter lets through.
    pub fn colour_share(&self, colour: Colour) -> f32 {
        self.colour_shares
            .get(colour as usize)
            .copied()
            .unwrap_or(1.0)
    }

    /// Every colour's share, unpainted first.
    pub fn colour_shares(&self) -> [f32; crate::groups::COLOURS + 1] {
        self.colour_shares
    }

    /// Sets what share of a colour continues. Unpainted hands never continue.
    pub fn set_colour_share(&mut self, colour: Colour, share: f64) {
        if colour as usize >= self.colour_shares.len() || colour == crate::groups::NONE {
            return;
        }
        self.colour_shares[colour as usize] = share.clamp(0.0, 1.0) as f32;
    }

    /// Whether a street's filter is applied.
    pub fn street_applied(&self, street: usize) -> bool {
        self.active()
            .streets
            .get(street)
            .is_some_and(Option::is_some)
    }

    /// How many combos are left after the filters up to and including a street.
    ///
    /// Cumulative, so the numbers on the buttons read down as a chain: what came
    /// into the flop, what survived it, what survived the turn. A street nobody
    /// has pressed yet shows what pressing it would leave, which is the question
    /// you ask before pressing rather than after.
    ///
    /// Counted against the board as it stands, so a card landing takes every
    /// combination that used it out of every number at once.
    pub fn street_count(&self, street: usize) -> f64 {
        let player = self.active();
        let mut range = player.range.clone();
        for index in 0..=street.min(2) {
            let set = match &player.streets[index] {
                Some(filter) => self.street_range(filter),
                // An earlier street nobody pressed narrows nothing; the one
                // being asked about is answered as if it had just been pressed.
                None if index == street => {
                    player
                        .groups
                        .painted_at_weights(&range, &self.cache, &self.colour_shares)
                }
                None => continue,
            };
            range = range.intersection(&set);
        }
        range.combo_count_excluding(self.dealt_elsewhere(self.active))
    }

    /// What one of the active seat's street filters currently keeps.
    fn street_range(&self, filter: &StreetFilter) -> Range {
        self.street_range_for(self.active, filter)
    }

    /// The same, for whichever seat the filter belongs to.
    fn street_range_for(&self, index: usize, filter: &StreetFilter) -> Range {
        match &filter.frozen {
            Some(range) => range.clone(),
            // Still the street it was pressed on, so it follows the painting.
            None => match self.players.get(index) {
                Some(player) => player.groups.painted_at_weights(
                    &player.range,
                    &self.cache,
                    &self.colour_shares,
                ),
                None => Range::empty(),
            },
        }
    }

    /// Keeps what each street's filter holds before the board moves to `dealt`.
    ///
    /// Called while the classification is still the one the filter was pressed
    /// against, which is the whole point: the painting has to be read on the
    /// board it was about.
    fn freeze_streets(&mut self, dealt: usize) {
        for index in 0..self.players.len() {
            let range = self.players[index].range.clone();
            let groups = self.players[index].groups.clone();
            let shares = self.colour_shares;
            for street in 0..3 {
                let Some(filter) = self.players[index].streets[street].as_mut() else {
                    continue;
                };
                if filter.frozen.is_none() && filter.dealt != dealt {
                    filter.frozen = Some(groups.painted_at_weights(&range, &self.cache, &shares));
                }
            }
        }
    }

    /// How many street filters the board has room for: flop, turn, river.
    pub fn streets_dealt(&self) -> usize {
        self.board.len().saturating_sub(2)
    }

    /// Paints the strongest `share` of the active range, by equity on this board.
    ///
    /// This is the slider talking to the same markers everything else uses, not
    /// a second kind of narrowing: it decides which hands carry a colour, and
    /// the matrix moves only when a street's filter is pressed. Returns `None`
    /// before the flop, where per-combo equity is sampled and far too noisy.
    /// Where the top-of-the-range slider has anywhere to stop.
    ///
    /// Equity across a range is a staircase, not a ramp: sorted strongest
    /// first, it holds a value over a run of hands and then drops. Between two
    /// steps there is nothing to choose - the same hands are painted either
    /// way - so the places worth stopping are the edges of the steps, returned
    /// here as the share of the range covered at each.
    ///
    /// The edges are where the equity *changes*, not where one more combination
    /// fits: half of a pair of aces is not a decision anyone makes, and the six
    /// of them share a value. Empty before a flop, where there is no equity to
    /// sort by and the slider has nothing to offer.
    pub fn equity_steps(&self) -> Vec<f32> {
        let villain = self.opponent_range().unwrap_or_else(Range::full);
        let mine = self.active().range.clone();
        let blocked = self.blocked_for_equity();
        let Some(equity) = equity::equity_by_combo(&mine, &villain, &self.board, blocked) else {
            return Vec::new();
        };
        let mut ordered: Vec<(f32, f32)> = Vec::new();
        let mut total = 0.0f64;
        for combo in self.cache.live().iter() {
            let weight = mine.get(combo);
            let value = equity.equity[combo.index() as usize];
            if weight <= 0.0 || value < 0.0 {
                continue;
            }
            ordered.push((value, weight));
            total += f64::from(weight);
        }
        if total <= 0.0 {
            return Vec::new();
        }
        ordered.sort_by(|a, b| b.0.total_cmp(&a.0));

        let mut steps = Vec::new();
        let mut covered = 0.0f64;
        let mut at = 0;
        while at < ordered.len() {
            let value = ordered[at].0;
            while at < ordered.len() && ordered[at].0 == value {
                covered += f64::from(ordered[at].1);
                at += 1;
            }
            steps.push((covered / total) as f32);
        }
        steps
    }

    /// Paints the strongest `share` of the active range, by equity on this board.
    ///
    /// This is the slider talking to the same markers everything else uses, not
    /// a second kind of narrowing: it decides which hands carry a colour, and
    /// the matrix moves only when a street's filter is pressed. Returns `None`
    /// before the flop, where per-combo equity is sampled and far too noisy.
    pub fn set_continue_by_equity(&mut self, share: f64) -> Option<Cut> {
        self.set_continue_between(0.0, share)
    }

    /// Paints a slice of the range, taken by equity, strongest first.
    ///
    /// `from` and `to` are shares of the range: the top fifth is `0.0..0.2`,
    /// the middle is `0.3..0.7`, the bottom quarter is `0.75..1.0`. One handle
    /// only ever reached the top, and the top is not the only part of a range
    /// worth looking at - a reader wants the hands that are neither good
    /// enough to raise nor bad enough to fold about as often.
    ///
    /// The whole of it is no slice at all, and puts back what was painted
    /// before.
    pub fn set_continue_between(&mut self, from: f64, to: f64) -> Option<Cut> {
        if !self.editable() {
            return None;
        }
        let (from, to) = if from <= to { (from, to) } else { (to, from) };
        let from = from.clamp(0.0, 1.0);
        let share = to.clamp(0.0, 1.0) - from;
        // Moving the slider a second time must not remember the state the first
        // move left behind: the marks to restore are the ones from before any of
        // this started.
        self.active_mut().painted = true;
        let restore = match self.active_mut().cut.take() {
            Some(previous) => previous.restore,
            None => self.active().groups.clone(),
        };
        if share >= 1.0 || share <= 0.0 {
            self.active_mut().groups = restore;
            return None;
        }
        let villain = self.opponent_range().unwrap_or_else(Range::full);
        let mine = self.active().range.clone();
        let blocked = self.blocked_for_equity();
        let Some(equity) = equity::equity_by_combo(&mine, &villain, &self.board, blocked) else {
            self.active_mut().groups = restore;
            return None;
        };

        // Strongest first, and take whole steps of the staircase: hands worth
        // the same are taken together or not at all. Three of a pair of aces is
        // not a decision anybody makes - it is an artefact of where a target
        // happened to land - so the slice runs to the end of the run it is in
        // rather than stopping inside it.
        let mut ordered: Vec<(Combo, f32, f32)> = Vec::new();
        let mut total = 0.0f64;
        for (combo, weight) in self.cache.live().iter().map(|c| (c, mine.get(c))) {
            let value = equity.equity[combo.index() as usize];
            if weight <= 0.0 || value < 0.0 {
                continue;
            }
            ordered.push((combo, value, weight));
            total += f64::from(weight);
        }
        if total <= 0.0 {
            self.active_mut().groups = restore;
            return None;
        }
        ordered.sort_by(|a, b| b.1.total_cmp(&a.1));

        let start = from * total;
        let target = (from + share) * total;
        let mut range = Range::empty();
        let mut covered = 0.0f64;
        let mut skipped = 0.0f64;
        let mut hand = None;
        let mut threshold = 1.0f32;
        // A hair under the target still counts as reaching it. The share
        // arrives from a slider that snapped to a step, and the step came back
        // through a float on the way: without this the sum lands a
        // ten-millionth short of its own boundary and the slice takes a whole
        // extra run of hands, so the panel disagreed with the slider.
        let reached = target - total * 1e-6;
        let begins = start - total * 1e-6;
        let mut at = 0;
        // Past the hands above the slice first, whole runs at a time, and then
        // through the slice itself the same way.
        while at < ordered.len() && skipped < begins {
            let value = ordered[at].1;
            while at < ordered.len() && ordered[at].1 == value {
                skipped += f64::from(ordered[at].2);
                at += 1;
            }
        }
        while at < ordered.len() && skipped + covered < reached {
            let value = ordered[at].1;
            while at < ordered.len() && ordered[at].1 == value {
                let (combo, _, weight) = ordered[at];
                range.set(combo, weight);
                covered += f64::from(weight);
                hand = Some(combo.to_string());
                at += 1;
            }
            threshold = value;
        }

        // The slider paints; it does not narrow. The top of the range gets the
        // held colour and the rest is unpainted, so the markers change - a
        // category the cut only half covers turns into a gear - and the matrix
        // only moves when a street's filter is pressed.
        let colour = self.colour;
        let mut groups = GroupSet::new();
        for (combo, weight) in range.iter() {
            if weight > 0.0 {
                groups.set(combo, colour, &self.cache);
            }
        }
        self.active_mut().groups = groups;

        let cut = Cut {
            share,
            covered: covered / total,
            threshold,
            from,
            hand,
            board: self.board.to_string(),
            restore,
            range,
        };
        self.active_mut().cut = Some(cut.clone());
        Some(cut)
    }

    /// The active seat's equity cut, if one is set.
    pub fn cut(&self) -> Option<&Cut> {
        self.active().cut.as_ref()
    }

    /// Drops the equity cut and puts back the marks it was laid over.
    ///
    /// A cut is a suggestion, not a commitment: whatever you had set up before
    /// reaching for the slider comes back exactly as it was.
    pub fn clear_cut(&mut self) {
        if !self.editable() {
            return;
        }
        if let Some(cut) = self.active_mut().cut.take() {
            self.active_mut().groups = cut.restore;
        }
    }

    /// How much of each matrix cell the cut keeps, for painting the selection.
    ///
    /// A share of the cell's own weight, not of its combo count, so a cell the
    /// cut keeps outright reads as kept outright however many combos it holds.
    pub fn cut_shares(&self) -> Option<[f32; NUM_CLASSES]> {
        let cut = self.active().cut.as_ref()?;
        let mut kept = [0.0f32; NUM_CLASSES];
        let mut held = [0.0f32; NUM_CLASSES];
        for combo in self.cache.live().iter() {
            let weight = self.active().range.get(combo);
            if weight <= 0.0 {
                continue;
            }
            let class = combo.class().index() as usize;
            held[class] += weight;
            kept[class] += cut.range.get(combo);
        }
        for (share, total) in kept.iter_mut().zip(held) {
            *share = if total > 0.0 { *share / total } else { 0.0 };
        }
        Some(kept)
    }

    /// What one combo is on this board, as statistic labels.
    ///
    /// Made rung first, then any draws, so a readout can say "top pair,
    /// flushdraw" rather than leaving the reader to work it out.
    pub fn describe_combo(&self, combo: Combo) -> Vec<&'static str> {
        self.cache
            .mask(combo)
            .iter()
            .filter(|stat| stat.def().block != StatBlock::Combination)
            .map(|stat| stat.def().label)
            .collect()
    }

    /// What the hands of one cell make, as a share of the cell for each
    /// statistic.
    ///
    /// The other way round from [`Self::highlight`], which takes a statistic
    /// and says which cells are about it. This takes a cell and says which
    /// statistics it is about - the question a reader asks by pointing at a
    /// hand rather than at a row.
    ///
    /// Over the combinations the board and the dead cards leave, and not over
    /// the range: the question is what the hand makes here, which has an answer
    /// whether or not the reader holds it. A cell every card of which is gone
    /// makes nothing, and says so with an empty list.
    pub fn class_stats(&self, class: HandClass) -> Vec<f32> {
        let gone = self
            .board
            .mask()
            .union(self.dead)
            .union(self.dealt_elsewhere(self.active));
        self.share_of(
            class
                .combos()
                .filter(|combo| !combo.mask().intersects(gone)),
        )
    }

    /// The same for one combination, where every share is nought or one.
    pub fn combo_stats(&self, combo: Combo) -> Vec<f32> {
        self.share_of(std::iter::once(combo))
    }

    fn share_of(&self, combos: impl Iterator<Item = Combo>) -> Vec<f32> {
        let mut counts = vec![0f32; stat_count()];
        let mut seen = 0f32;
        for combo in combos {
            seen += 1.0;
            for stat in self.cache.mask(combo).iter() {
                counts[stat.index() as usize] += 1.0;
            }
        }
        if seen == 0.0 {
            return Vec::new();
        }
        for count in &mut counts {
            *count /= seen;
        }
        counts
    }

    /// Unpaints everything on the active seat and lifts every street filter.
    pub fn clear_filters(&mut self) {
        self.active_mut().groups.clear();
        self.active_mut().painted = true;
        self.active_mut().streets = [None, None, None];
        self.active_mut().cut = None;
    }

    // ----- output ---------------------------------------------------------------

    /// The per-board classification cache.
    pub fn stats(&self) -> &ComboStats {
        &self.cache
    }

    /// The active range after the filters, if they are switched on.
    pub fn effective_range(&self) -> Range {
        self.narrowed()
    }

    /// The range once every applied street filter has had its say.
    ///
    /// Each street froze the hands that carried a colour when its button was
    /// pressed, so the range going forward is the intersection: what continued
    /// on the flop, and then on the turn, and then on the river.
    fn narrowed(&self) -> Range {
        self.narrowed_for(self.active)
    }

    /// The same, for whichever seat is asked about.
    ///
    /// A filter belongs to the seat that set it, not to whichever seat happens
    /// to be selected - otherwise switching seats to look at the equity would
    /// show the one you just narrowed at its full width.
    fn narrowed_for(&self, index: usize) -> Range {
        let Some(player) = self.players.get(index) else {
            return Range::empty();
        };
        let mut range = player.range.clone();
        for filter in player.streets.iter().flatten() {
            range = range.intersection(&self.street_range_for(index, filter));
        }
        // A hand somebody else is holding is a hand this seat is not, so it
        // comes out of the range here - once, where every count, every cell and
        // every equity downstream reads from.
        range.without_cards(self.dealt_elsewhere(index))
    }

    /// The cards that are out of the deck as far as one seat is concerned.
    ///
    /// The board and the dead cards for everyone, plus the hands the other
    /// seats have been dealt - but never a seat's own hand, which is the one it
    /// is holding.
    pub fn dealt_elsewhere(&self, seat: usize) -> CardSet {
        let mut blocked = self.board.mask().union(self.dead);
        for (index, player) in self.players.iter().enumerate() {
            if index == seat {
                continue;
            }
            if let Some(hand) = player.hand {
                blocked = blocked.union(hand.mask());
            }
        }
        blocked
    }

    /// Every card the board can no longer use.
    ///
    /// The dead cards and every hand dealt at the table, the reader's own
    /// included: a flop cannot contain a card somebody is already holding. The
    /// counting, the sampling and the pass over the flops all ask this rather
    /// than asking for the dead cards, which was how dealing yourself a hand
    /// left the panel still counting all 22,100.
    pub fn off_the_deck(&self) -> CardSet {
        self.dead.union(self.dealt_hands())
    }

    /// Every card a seat has been dealt, for the card grids to grey out.
    pub fn dealt_hands(&self) -> CardSet {
        self.players
            .iter()
            .filter_map(|player| player.hand)
            .fold(CardSet::EMPTY, |cards, hand| cards.union(hand.mask()))
    }

    /// Whether any street filter is applied.
    pub fn filters_enabled(&self) -> bool {
        self.active().streets.iter().any(Option::is_some)
    }

    /// Weighted combos of the active range that are playable against this board.
    ///
    /// The range's own size counts every combo; this drops the ones the board and
    /// the dead cards have already taken, which is the denominator every
    /// percentage in the statistics panel uses.
    pub fn live_combos(&self) -> f64 {
        let blocked = self.board.mask().union(self.dead);
        self.active().range.combo_count_excluding(blocked)
    }

    /// The statistics panel for the active seat.
    pub fn breakdown(&self) -> Breakdown {
        breakdown::breakdown(
            &self.effective_range(),
            &self.cache,
            self.options,
            self.mode,
        )
    }

    /// The statistics panel for one seat, whichever seat is selected.
    ///
    /// Reading "my range hits top pair 16% of the time" is half a thought; the
    /// other half is what the other range does on the same board. Switching
    /// seats to find out means holding the first number in your head, so the
    /// panel can show a second column instead.
    pub fn breakdown_for(&self, seat: usize, within: Option<StatMask>) -> Option<Breakdown> {
        if seat >= self.players.len() {
            return None;
        }
        let range = self.narrowed_for(seat);
        Some(match within {
            Some(mask) => {
                breakdown::breakdown_within(&range, &self.cache, self.options, self.mode, mask)
            }
            None => breakdown::breakdown(&range, &self.cache, self.options, self.mode),
        })
    }

    /// Which seat the panel is comparing against, if any.
    pub fn compare_seat(&self) -> Option<usize> {
        self.compare
            .filter(|seat| *seat < self.players.len() && *seat != self.active)
    }

    /// Sets the seat to compare against, or clears the comparison.
    ///
    /// A seat cannot be compared with itself, and the choice follows the seats
    /// rather than the selection: picking B while looking at A, then switching
    /// to B, compares B against B - which is nothing - so the panel drops the
    /// column until another seat is picked.
    pub fn set_compare_seat(&mut self, seat: Option<usize>) {
        self.compare = seat.filter(|seat| *seat < self.players.len());
    }

    /// The second column, for the seat being compared against.
    pub fn compare_breakdown(&self, within: Option<StatMask>) -> Option<Breakdown> {
        self.breakdown_for(self.compare_seat()?, within)
    }

    /// The statistics panel restricted to one statistic, as hovering does.
    pub fn breakdown_within(&self, mask: StatMask) -> Breakdown {
        breakdown::breakdown_within(
            &self.effective_range(),
            &self.cache,
            self.options,
            self.mode,
            mask,
        )
    }

    /// Per-cell weights for the matrix, optionally restricted to one statistic.
    pub fn highlight(&self, mask: StatMask) -> [f32; NUM_CLASSES] {
        breakdown::highlight(&self.effective_range(), &self.cache, mask)
    }

    /// Which statistics count as the range having hit, in the preflop mode.
    pub fn checkmarks(&self) -> StatMask {
        self.checkmarks
    }

    /// Adds or removes a preflop checkmark.
    pub fn toggle_checkmark(&mut self, stat: StatId) {
        if self.checkmarks.has(stat) {
            self.checkmarks.clear(stat);
        } else {
            self.checkmarks.set(stat);
        }
    }

    /// Clears every preflop checkmark.
    pub fn clear_checkmarks(&mut self) {
        self.checkmarks = StatMask::EMPTY;
    }

    /// Which flops a pass over them looks at.
    pub fn flop_filter(&self) -> FlopFilter {
        self.flop_filter
    }

    /// Adds or removes one group of flops from what a pass looks at.
    ///
    /// `false` when there is no such group, which is the only way to get it
    /// wrong: everything else is a tick.
    pub fn toggle_flop_group(&mut self, axis: &str, group: &str) -> bool {
        self.flop_filter.toggle(axis, group)
    }

    /// Puts every flop back in.
    pub fn clear_flop_filter(&mut self) {
        self.flop_filter.clear();
    }

    /// How many flops a pass would look at, after the dead cards.
    pub fn flop_filter_count(&self) -> u64 {
        self.flop_filter.count(self.off_the_deck())
    }

    /// How often each statistic comes with each other one.
    pub fn overlap(&self) -> OverlapMatrix {
        breakdown::overlap(&self.effective_range(), &self.cache, self.options)
    }

    /// The active range classified against every one of the 22,100 flops.
    ///
    /// This is the one calculation that is not instant - a wide range is tens of
    /// millions of classifications - so it is run on request rather than on every
    /// keystroke, which is also how Flopzilla does it.
    pub fn preflop(&self) -> PreflopBreakdown {
        self.preflop_for(self.active)
    }

    /// Runs the pass for every seat that still needs one.
    ///
    /// One press rather than one per seat. A reader comparing two ranges wants
    /// both answered, and the round trip - go back to the other seat, press
    /// again, come back - is the reader doing by hand what the button is for.
    /// Passes already standing cost nothing to ask for again, so this is only
    /// as slow as what is actually outstanding.
    pub fn preflop_all(&self) {
        if !self.board.is_empty() {
            return;
        }
        for seat in 0..self.players.len() {
            if self.preflop_outstanding_for(seat) {
                self.preflop_for(seat);
            }
        }
    }

    /// How many seats have a pass to run, or equity to work out alongside one.
    pub fn preflop_outstanding(&self) -> usize {
        if !self.board.is_empty() {
            return 0;
        }
        (0..self.players.len())
            .filter(|seat| self.preflop_outstanding_for(*seat))
            .count()
    }

    /// Whether one seat is waiting on a pass, either half of it.
    ///
    /// Two halves that go stale apart: the breakdown is about the range and
    /// the flops, the equity about that range against another one. A seat with
    /// nothing in it is waiting for nothing.
    fn preflop_outstanding_for(&self, seat: usize) -> bool {
        let mine = self.narrowed_for(seat);
        if mine.is_empty() {
            return false;
        }
        let key = self.preflop_key_for(seat);
        if !self
            .preflop_cache
            .borrow()
            .iter()
            .any(|(cached, _)| *cached == key)
        {
            return true;
        }
        let villain = self.raw_opponent_range_for(seat);
        !villain.is_empty()
            && self
                .preflop_equity_with(&mine, &villain, self.blocked_for_equity_for(seat))
                .is_none()
    }

    /// The same as [`Session::preflop`], about whichever seat asked.
    fn preflop_for(&self, seat: usize) -> PreflopBreakdown {
        let key = self.preflop_key_for(seat);
        let standing = self.preflop_cached_for(seat);
        let breakdown = standing.clone().unwrap_or_else(|| {
            preflop::over_flops(
                &self.players[seat].range,
                self.off_the_deck(),
                self.dealt_elsewhere(seat),
                self.options,
                self.checkmarks,
                self.flop_filter,
            )
        });

        // The equity views ask the same question of the same flops - what is a
        // hand worth, given a board out of this set - so they are answered here
        // rather than behind a second button and a second wait.
        self.work_out_preflop_equity_for(seat);

        let mut cache = self.preflop_cache.borrow_mut();
        cache.retain(|(cached, _)| *cached != key);
        if cache.len() >= Self::MAX_SEATS {
            cache.remove(0);
        }
        cache.push((key, breakdown.clone()));
        breakdown
    }

    /// The pass for the seat as it stands, if one has already been run.
    ///
    /// `None` means nobody has looked yet - at this range, on these dead cards,
    /// over these flops. The panel shows that rather than a stale answer.
    pub fn preflop_cached(&self) -> Option<PreflopBreakdown> {
        self.preflop_cached_for(self.active)
    }

    /// The same, about a seat that is not necessarily the selected one.
    fn preflop_cached_for(&self, seat: usize) -> Option<PreflopBreakdown> {
        let key = self.preflop_key_for(seat);
        let cache = self.preflop_cache.borrow();
        let (_, breakdown) = cache.iter().find(|(cached, _)| *cached == key)?;
        // The checkmarks may have moved since; the shape has not.
        let mut result = breakdown.clone();
        result.hit = result.hit_for(self.checkmarks);
        Some(result)
    }

    /// Which hands are behind each tier of the last pass, as weights.
    ///
    /// Laid out as `combo * 4 + tier`, and empty when no pass is standing for
    /// the range as it is. Read separately from the breakdown because it is
    /// five thousand numbers that only the pie asks for.
    pub fn preflop_tiers(&self) -> Vec<f32> {
        let key = self.preflop_key();
        let cache = self.preflop_cache.borrow();
        cache
            .iter()
            .find(|(cached, _)| *cached == key)
            .map(|(_, breakdown)| breakdown.tiers.clone())
            .unwrap_or_default()
    }

    /// Everything neither side of a per-hand equity calculation can hold.
    ///
    /// The board, the dead cards, and any hand dealt at a seat that is not one
    /// of the two. Not the two sides' own hands: a dealt hand *is* the range on
    /// that side, and a range that crossed its own cards out would have nothing
    /// left to measure - which is why the equity views went blank against a
    /// dealt hand. The removal between the two sides is done combination by
    /// combination inside the calculation, which is more exact than crossing
    /// cards out in advance.
    fn blocked_for_equity(&self) -> CardSet {
        self.blocked_for_equity_for(self.active)
    }

    /// The same, about a seat that is not necessarily the selected one.
    fn blocked_for_equity_for(&self, mine: usize) -> CardSet {
        let versus = self.versus_seat().filter(|seat| *seat != mine);
        let mut blocked = self.board.mask().union(self.dead);
        for (index, player) in self.players.iter().enumerate() {
            // With nobody singled out, the opponent is every other seat, so
            // there is no third party left to block.
            let inside = index == mine || versus.is_none_or(|seat| seat == index);
            if inside {
                continue;
            }
            if let Some(hand) = player.hand {
                blocked = blocked.union(hand.mask());
            }
        }
        blocked
    }

    /// The preflop equity of one range against another, if it has been worked
    /// out and still says something about the ranges as they are.
    fn preflop_equity_for(&self, hero: &Range, villain: &Range) -> Option<ComboEquity> {
        self.preflop_equity_with(hero, villain, self.blocked_for_equity())
    }

    /// The same, for a pass whose dead cards are another seat's.
    fn preflop_equity_with(
        &self,
        hero: &Range,
        villain: &Range,
        blocked: CardSet,
    ) -> Option<ComboEquity> {
        let wanted = self.preflop_equity_key(hero, villain, blocked);
        let cache = self.preflop_equity.borrow();
        let (_, equity) = cache.iter().find(|(cached, _)| *cached == wanted)?;
        Some(equity.clone())
    }

    /// What one direction of a preflop equity pass is about: the two ranges,
    /// the dead cards and the flops it was over.
    ///
    /// The pair rather than the seat, so that looking at the same two ranges
    /// from the other side finds the answer that is already there.
    fn preflop_equity_key(&self, hero: &Range, villain: &Range, blocked: CardSet) -> u64 {
        self.fingerprint(&[hero, villain], 9)
            ^ blocked.bits()
            ^ self
                .flop_filter
                .bits()
                .iter()
                .fold(0u64, |hash, axis| hash << 8 | u64::from(*axis))
    }

    /// Files one direction of a pass.
    fn keep_preflop_equity(
        &self,
        hero: &Range,
        villain: &Range,
        blocked: CardSet,
        value: ComboEquity,
    ) {
        let key = self.preflop_equity_key(hero, villain, blocked);
        let mut cache = self.preflop_equity.borrow_mut();
        cache.retain(|(cached, _)| *cached != key);
        // Both directions of a few pairs, and no more: each is a megabyte of
        // nothing once the reader has moved on.
        if cache.len() >= 2 * Self::MAX_SEATS {
            cache.remove(0);
        }
        cache.push((key, value));
    }

    /// Runs the per-hand equity over the flops the ticks leave, for one seat.
    ///
    /// Both directions, because the graph draws both curves - and because the
    /// other seat's question is this one backwards, so filing both is what
    /// lets switching seats find the answer already worked out.
    fn work_out_preflop_equity_for(&self, seat: usize) {
        if !self.board.is_empty() {
            return;
        }
        let mine = self.narrowed_for(seat);
        let villain = self.raw_opponent_range_for(seat);
        if mine.is_empty() || villain.is_empty() {
            return;
        }
        let dead = self.blocked_for_equity_for(seat);
        let filter = self.flop_filter;
        if self.preflop_equity_with(&mine, &villain, dead).is_none() {
            if let Some(ours) = equity::equity_by_combo_preflop(&mine, &villain, dead, filter) {
                self.keep_preflop_equity(&mine, &villain, dead, ours);
            }
        }
        if self.preflop_equity_with(&villain, &mine, dead).is_none() {
            if let Some(theirs) = equity::equity_by_combo_preflop(&villain, &mine, dead, filter) {
                self.keep_preflop_equity(&villain, &mine, dead, theirs);
            }
        }
    }

    /// The headline of the last pass, re-read for the checkmarks as they stand.
    ///
    /// `None` when there is no pass to re-read - the range or the dead cards
    /// have moved since, and the honest answer is that nobody has looked yet.
    pub fn preflop_hit(&self) -> Option<f64> {
        let key = self.preflop_key();
        let cache = self.preflop_cache.borrow();
        let (_, breakdown) = cache.iter().find(|(cached, _)| *cached == key)?;
        Some(breakdown.hit_for(self.checkmarks))
    }

    /// What a pass over all the flops depends on. Not the checkmarks: those
    /// pick what to ask of the answer rather than changing it.
    fn preflop_key(&self) -> u64 {
        self.preflop_key_for(self.active)
    }

    /// The same, about a seat that is not necessarily the selected one.
    fn preflop_key_for(&self, seat: usize) -> u64 {
        let mut hash = self.fingerprint(&[&self.players[seat].range], 7);
        hash ^= self.off_the_deck().bits();
        hash = hash.wrapping_mul(0x100_0000_01b3);
        hash ^= u64::from(self.options.one_card_backdoor_flushdraw);
        for bits in self.flop_filter.bits() {
            hash ^= u64::from(bits);
            hash = hash.wrapping_mul(0x100_0000_01b3);
        }
        hash
    }

    /// How often each kind of flop comes, given the dead cards and the ticks.
    pub fn flop_breakdown(&self) -> FlopBreakdown {
        flops::filtered_breakdown(self.off_the_deck(), self.flop_filter)
    }

    /// Equity for each combo of the active range, for the equity matrix and graph.
    ///
    /// Preflop this is only ever the answer to a question the reader has asked:
    /// there is no board to enumerate, the estimate costs about a second, and
    /// everything here is read on every redraw. So before a flop it hands back
    /// what the pass over the flops left and nothing else.
    pub fn equity_by_combo(&self) -> Option<ComboEquity> {
        let mine = self.effective_range();
        let villain = self.opponent_range().unwrap_or_default();
        if self.board.is_empty() {
            return self.preflop_equity_for(&mine, &villain);
        }
        let key = self.by_combo_key(&mine, &villain);
        if let Some(cached) = &self.equity_cache.borrow().by_combo {
            if cached.key == key {
                return cached.value.clone();
            }
        }
        let result = (!villain.is_empty())
            .then(|| {
                equity::equity_by_combo(&mine, &villain, &self.board, self.blocked_for_equity())
            })
            .flatten();
        self.equity_cache.borrow_mut().by_combo = Some(Cached {
            key,
            value: result.clone(),
        });
        result
    }

    fn by_combo_key(&self, mine: &Range, villain: &Range) -> u64 {
        self.fingerprint(&[mine, villain], 1) ^ self.dealt_hands().bits()
    }

    /// Whether a preflop equity pass is standing for what is on the table.
    ///
    /// Goes false by itself when either range or the ticked flops move, because
    /// the answer was about what it was run on - the same promise the rest of
    /// the pass makes.
    pub fn preflop_equity_ready(&self) -> bool {
        self.board.is_empty()
            && self
                .preflop_equity_for(
                    &self.effective_range(),
                    &self.opponent_range().unwrap_or_default(),
                )
                .is_some()
    }

    /// Whether the pass would have per-hand equity to work out as well.
    ///
    /// What it costs, too: the panel that decides whether to run a pass without
    /// being asked has to know that this is riding along with it.
    pub fn preflop_equity_work(&self) -> f64 {
        if !self.board.is_empty() {
            return 0.0;
        }
        let mine = self.effective_range();
        let villain = self.opponent_range().unwrap_or_default();
        if mine.is_empty() || villain.is_empty() {
            return 0.0;
        }
        let dead = self.blocked_for_equity();
        let boards = f64::from(equity::preflop_boards(&mine, &villain, dead));
        // Both sides are worked out, and each board evaluates both ranges.
        2.0 * boards * (mine.live(dead).len() + villain.live(dead).len()) as f64
    }

    /// Per-combo equity for the seat the active range is measured against.
    ///
    /// The other side of the same enumeration, so a graph can draw both curves
    /// and show which range is ahead of which. `None` when there is no distinct
    /// opponent - a lone range is measured against itself, and plotting that
    /// twice says nothing.
    pub fn opponent_equity_by_combo(&self) -> Option<ComboEquity> {
        if self.board.is_empty() {
            let villain = self.raw_opponent_range();
            return self.preflop_equity_for(&villain, &self.effective_range());
        }
        let key = self.fingerprint(&[&self.effective_range(), &self.raw_opponent_range()], 2);
        if let Some(cached) = &self.equity_cache.borrow().opponent {
            if cached.key == key {
                return cached.value.clone();
            }
        }
        let result = self.compute_opponent_equity();
        self.equity_cache.borrow_mut().opponent = Some(Cached {
            key,
            value: result.clone(),
        });
        result
    }

    fn compute_opponent_equity(&self) -> Option<ComboEquity> {
        let villain = self.raw_opponent_range();
        if villain.is_empty() {
            return None;
        }
        equity::equity_by_combo(
            &villain,
            &self.effective_range(),
            &self.board,
            self.blocked_for_equity(),
        )
    }

    /// Deals a random flop from the ones the ticked groups leave.
    ///
    /// With nothing ticked that is every flop, which is the plain random deal.
    /// With something ticked it is a sample of what the reader said they were
    /// interested in - deal, look, deal again, without leaving the set.
    ///
    /// Returns whether a flop was found; nothing is left when the dead cards
    /// and the ticks between them rule out every board.
    pub fn deal_flop(&mut self) -> bool {
        let Some(flop) =
            flops::sample_matching(self.flop_filter, self.off_the_deck(), &mut self.rng)
        else {
            return false;
        };
        self.board = flop;
        self.rebuild();
        true
    }

    /// Deals a random flop from one bucket of the flop breakdown.
    ///
    /// Returns whether a flop was found, which fails only for a bucket the dead
    /// cards have emptied or a key that names no bucket.
    pub fn deal_flop_from(&mut self, axis: &str, group: &str) -> bool {
        let Some(flop) = flops::sample(axis, group, self.off_the_deck(), &mut self.rng) else {
            return false;
        };
        self.board = flop;
        self.rebuild();
        true
    }

    /// The one hand the active seat holds, if it holds exactly one.
    ///
    /// A hand is a range with one combination in it, which the seats could
    /// always express and nothing ever read. Reading it here means the tools
    /// that are about one hand can be pointed at a seat - editable, shareable,
    /// and measured against whichever other seat was chosen - rather than only
    /// at the two cards in the dead-card slots.
    pub fn lone_combo(&self) -> Option<Combo> {
        self.lone_combo_for(self.active)
    }

    /// The hand the per-hand views are about, and the seat it is sitting at.
    ///
    /// The selected seat when it holds a hand; failing that the only hand at
    /// the table, because dealing one no longer moves the reader onto it.
    /// Several hands and nobody has said which, so neither does this.
    fn hand_in_question(&self) -> Option<(usize, Combo)> {
        if let Some(hand) = self.lone_combo() {
            return Some((self.active, hand));
        }
        let mut hands = self
            .players
            .iter()
            .enumerate()
            .filter_map(|(seat, player)| player.hand.map(|hand| (seat, hand)));
        let first = hands.next()?;
        hands.next().is_none().then_some(first)
    }

    /// The only hand dealt at the table, if exactly one has been.
    pub fn only_hand(&self) -> Option<Combo> {
        self.hand_in_question().map(|(_, hand)| hand)
    }

    /// The one hand a seat holds, if it holds exactly one.
    /// The known hand a seat holds, if it is a hand rather than a range.
    pub fn lone_combo_for(&self, seat: usize) -> Option<Combo> {
        self.players.get(seat).and_then(|player| player.hand)
    }

    /// How each remaining card would change the hand's equity.
    ///
    /// The hand is the one the active seat holds, when it holds one. Failing
    /// that it is the only hand at the table, wherever it is sitting: dealing
    /// a hand no longer moves the reader onto it, so insisting they go there
    /// first would mean a view that says "deal a hand" to somebody who just
    /// did. Several hands is a question this cannot answer, and it says so by
    /// answering nothing.
    pub fn hotness(&self) -> Option<Vec<HotCard>> {
        let (seat, hand) = self.hand_in_question()?;
        // Measured against everyone but the seat the hand is sitting at. Not
        // everyone but the *selected* seat: with the reader on their range and
        // the hand dealt beside it, that field held the hand itself, and a hand
        // against itself is a matchup that cannot be dealt - which came out as
        // a column of noughts.
        let villain = self.raw_opponent_range_for(seat);
        (!villain.is_empty())
            .then(|| equity::hotness(hand, &villain, &self.board, self.dealt_elsewhere(seat)))
            .flatten()
    }

    /// Which seat the per-hand equity views are measured against, if one.
    ///
    /// `None` is every other seat taken together. A seat that has emptied out,
    /// or that has become the selected one, is no choice at all and falls back
    /// to the field.
    ///
    /// With one other range in play the field *is* that range, and saying so
    /// keeps the graph drawing it in that seat's colour under its own name
    /// rather than as an anonymous crowd of one.
    pub fn versus_seat(&self) -> Option<usize> {
        let named = self
            .versus
            .filter(|seat| *seat != self.active && *seat < self.players.len())
            .filter(|seat| !self.narrowed_for(*seat).is_empty());
        if named.is_some() {
            return named;
        }
        let mut others = (0..self.players.len())
            .filter(|seat| *seat != self.active)
            .filter(|seat| !self.narrowed_for(*seat).is_empty());
        let only = others.next();
        others.next().is_none().then_some(only).flatten()
    }

    /// Chooses the seat to measure against, or `None` for all of them at once.
    pub fn set_versus_seat(&mut self, seat: Option<usize>) {
        self.versus = seat.filter(|seat| *seat < self.players.len());
    }

    /// The range the active seat is measured against.
    ///
    /// One named seat is that seat. Everything else is every other seat's range
    /// laid over one another, which answers "how do I do against a hand from
    /// this table" - a different question from the multiway pot in the readout,
    /// where the hand has to beat all of them at once rather than one of them.
    /// Per-hand numbers are exact for two ranges and would have to be sampled
    /// for three, so the views that paint a number onto every combination ask
    /// the question that can be answered exactly.
    fn opponent_range(&self) -> Option<Range> {
        let range = self.raw_opponent_range();
        (!range.is_empty()).then_some(range)
    }

    /// The share of the active range that survives its filters.
    pub fn pass_fraction(&self) -> f64 {
        let blocked = self.board.mask().union(self.dead);
        let before = self.active().range.combo_count_excluding(blocked);
        if before <= 0.0 {
            return 0.0;
        }
        self.narrowed().combo_count_excluding(blocked) / before
    }

    /// The equity readout: every seat that has hands, against every other.
    ///
    /// A hand is a seat holding one combination, so measuring a hand against a
    /// range needs nothing else - and the dead cards are left to mean the one
    /// thing they say, which is that those cards are not in the deck.
    pub fn equity(&self) -> Option<EquityReport> {
        // Every seat as its own filters leave it. The key has to name the same
        // things the answer is computed from, and none of them depend on which
        // seat is selected.
        let seats: Vec<Range> = (0..self.players.len())
            .map(|index| self.narrowed_for(index))
            .collect();
        let refs: Vec<&Range> = seats.iter().collect();
        let key = self.fingerprint(&refs, 0);
        let mut cache = self.equity_cache.borrow_mut();
        if let Some(cached) = &cache.report {
            if cached.key == key {
                return cached.value.clone();
            }
        }
        let report = self.compute_equity();
        cache.report = Some(Cached {
            key,
            value: report.clone(),
        });
        report
    }

    fn compute_equity(&self) -> Option<EquityReport> {
        // Each seat as its own filters leave it: a continuing range is the whole
        // point of setting one, so measuring the full width would be measuring
        // a hand nobody is going to be holding. A seat its filters have emptied
        // is not in the pot at all.
        let seats: Vec<Range> = self
            .equity_seats()
            .into_iter()
            .map(|index| self.narrowed_for(index))
            .collect();
        let refs: Vec<&Range> = seats.iter().collect();
        equity::range_vs_ranges(&refs, &self.board, self.dead)
    }

    /// A cheap hash of the board and some ranges.
    ///
    /// A fingerprint rather than a revision counter bumped by every mutator:
    /// one missed mutator would leave a stale number on screen, and a stale
    /// equity is worse than a slow one. FNV-1a over a few kilobytes of weights
    /// is microseconds; the enumeration it saves is tens of milliseconds.
    fn fingerprint(&self, ranges: &[&Range], salt: u64) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |bytes: &[u8]| {
            for byte in bytes {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x100_0000_01b3);
            }
        };
        eat(&salt.to_le_bytes());
        eat(self.board.to_string().as_bytes());
        eat(&self.dead.bits().to_le_bytes());
        for range in ranges {
            eat(&[0xff]);
            for (combo, weight) in range.iter() {
                if weight > 0.0 {
                    eat(&combo.index().to_le_bytes());
                    eat(&weight.to_le_bytes());
                }
            }
        }
        hash
    }

    /// The other seat's range, as narrow as its own filters make it.
    fn raw_opponent_range(&self) -> Range {
        self.raw_opponent_range_for(self.active)
    }

    /// The same, for a seat that is not necessarily the selected one.
    fn raw_opponent_range_for(&self, mine: usize) -> Range {
        if let Some(seat) = self.versus_seat().filter(|seat| *seat != mine) {
            return self.narrowed_for(seat);
        }
        (0..self.players.len())
            .filter(|seat| *seat != mine)
            .map(|seat| self.narrowed_for(seat))
            .fold(Range::empty(), |field, range| field.union(&range))
    }

    // ----- saving ---------------------------------------------------------------

    /// A serialisable copy of the session.
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            version: Snapshot::VERSION,
            board: self.board.to_string(),
            dead: self.dead.to_string(),
            active: self.active,
            flags: [
                to_digit(
                    Ranking::ALL
                        .iter()
                        .position(|r| *r == self.ranking)
                        .unwrap_or(0),
                ),
                if self.mode == BreakdownMode::Cumulative {
                    '1'
                } else {
                    '0'
                },
                if self.options.one_card_backdoor_flushdraw {
                    '1'
                } else {
                    '0'
                },
                to_digit(self.colour as usize),
            ]
            .into_iter()
            .collect(),
            shares: if self.colour_shares.iter().all(|share| *share >= 1.0) {
                String::new()
            } else {
                self.colour_shares
                    .iter()
                    .map(|share| to_digit((share * 20.0).round() as usize))
                    .collect()
            },
            compare: self.compare,
            versus: self.versus,
            flop_groups: self.flop_filter.to_code(to_digit),
            players: self
                .players
                .iter()
                .map(|player| PlayerSnapshot {
                    name: String::new(),
                    hand: player.hand.map(|combo| combo.to_string()),
                    range: player.range.to_packed_notation(),
                    // Both layers: the categories in the order they were
                    // painted, and the hands that went their own way.
                    groups: save_groups(&player.groups),
                    // Trailing nothings need not travel: a filter nobody
                    // pressed reads back as one nobody pressed whether the slot
                    // is there or not.
                    streets: trim_trailing_nothing(
                        player
                            .streets
                            .iter()
                            .map(|street| {
                                street.as_ref().map(|filter| StreetSnapshot {
                                    dealt: filter.dealt,
                                    frozen: filter.frozen.as_ref().map(Range::to_packed_notation),
                                })
                            })
                            .collect(),
                    ),
                    painted: player.painted,
                    cut_share: player.cut.as_ref().map(|cut| cut.share),
                    cut_restore: player.cut.as_ref().map(|cut| save_groups(&cut.restore)),
                })
                .collect(),
        }
    }

    /// Rebuilds a session from a snapshot, ignoring statistics it does not know.
    pub fn restore(snapshot: &Snapshot) -> Result<Self, ParseError> {
        let mut session = Self::new();
        session.board = Board::parse(&snapshot.board)?;
        session.dead = CardSet::parse(&snapshot.dead)?.difference(session.board.mask());
        // One character each, in the order they are written. Anything missing
        // or unrecognised leaves that setting at its default rather than
        // stopping the link from opening at all.
        let mut flags = snapshot.flags.chars();
        session.ranking = flags
            .next()
            .and_then(from_digit)
            .and_then(|at| Ranking::ALL.get(at).copied())
            .unwrap_or_default();
        session.mode = match flags.next() {
            Some('1') => BreakdownMode::Cumulative,
            _ => BreakdownMode::Absolute,
        };
        session.options.one_card_backdoor_flushdraw = flags.next() == Some('1');
        session.colour = flags
            .next()
            .and_then(from_digit)
            .map(in_palette)
            .filter(|colour| *colour != crate::groups::NONE)
            .unwrap_or(DEFAULT_COLOUR);
        session.compare = snapshot.compare;
        session.versus = snapshot.versus;
        session.flop_filter = FlopFilter::from_code(&snapshot.flop_groups, from_digit);
        for (slot, digit) in session
            .colour_shares
            .iter_mut()
            .zip(snapshot.shares.chars())
        {
            if let Some(twentieths) = from_digit(digit) {
                *slot = (twentieths as f32 / 20.0).clamp(0.0, 1.0);
            }
        }

        if !snapshot.players.is_empty() {
            session.players.clear();
            let mut letter = 0u8;
            for saved in &snapshot.players {
                let mut player = Player::new(saved.name.clone());
                // A hand comes back as a hand, or the link would reopen with an
                // editable range that looks the same and behaves differently.
                player.hand = saved
                    .hand
                    .as_deref()
                    .and_then(|text| Combo::parse(text).ok());
                // The name is worked out rather than carried: a hand is named
                // after its cards, and a range after its place in the row.
                if player.name.is_empty() {
                    player.name = match player.hand {
                        Some(hand) => hand.to_string(),
                        None => format!("Range {}", char::from(b'A' + letter)),
                    };
                }
                if player.hand.is_none() {
                    letter += 1;
                }
                player.range = Range::parse(&saved.range)?;
                player.painted = saved.painted;
                for (street, saved) in saved.streets.iter().enumerate().take(3) {
                    player.streets[street] = match saved {
                        Some(filter) => Some(StreetFilter {
                            dealt: filter.dealt,
                            frozen: match &filter.frozen {
                                Some(text) => Some(Range::parse(text)?),
                                None => None,
                            },
                        }),
                        None => None,
                    };
                }
                session.players.push(player);
            }
        }
        session.active = snapshot.active.min(session.players.len().saturating_sub(1));
        // A link carries a range, not a pair of handle positions: park them on
        // what came back, so the slider describes it rather than describing
        // whatever it last described in some other session.
        let restored_to = session.active;
        for seat in 0..session.players.len() {
            session.active = seat;
            session.park_slider();
        }
        session.active = restored_to;
        session.rebuild();

        // The grouping has to be read after the board, because which hands a
        // category holds depends on it. `rebuild` has just put the default in
        // for any seat nobody had painted, which is exactly right for those.
        for (index, saved) in snapshot.players.iter().enumerate() {
            if index >= session.players.len() || !saved.painted {
                continue;
            }
            session.players[index].groups = load_groups(&saved.groups, &session.cache);
        }

        // The slice a cut chose is recomputed rather than stored - it depends on
        // the board and on the other seat, both of which are only in place now -
        // but the painting it replaced is saved, because nothing can re-derive it.
        let was_active = session.active;
        for (index, saved) in snapshot.players.iter().enumerate() {
            let Some(share) = saved.cut_share else {
                continue;
            };
            if index >= session.players.len() {
                break;
            }
            session.active = index;
            let painted = session.active().groups.clone();
            session.set_continue_by_equity(share);
            if let Some(saved) = &saved.cut_restore {
                let restore = load_groups(saved, &session.cache);
                if let Some(cut) = session.active_mut().cut.as_mut() {
                    cut.restore = restore;
                }
            }
            // Re-running the cut repaints; the saved painting is the truth,
            // because it may have been edited by hand since.
            session.active_mut().groups = painted;
        }
        session.active = was_active;
        Ok(session)
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

/// A saved session, as written to a file or packed into a shareable link.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct Snapshot {
    /// Format version, so a link written by a future build can be refused
    /// rather than half read.
    #[cfg_attr(feature = "serde", serde(rename = "v", alias = "version"))]
    pub version: u32,
    /// The board, as text.
    #[cfg_attr(feature = "serde", serde(rename = "b", alias = "board"))]
    pub board: String,
    /// The dead cards, as text.
    #[cfg_attr(feature = "serde", serde(rename = "d", alias = "dead"))]
    #[cfg_attr(
        feature = "serde",
        serde(default, skip_serializing_if = "String::is_empty")
    )]
    pub dead: String,
    /// Which seat was selected.
    #[cfg_attr(feature = "serde", serde(rename = "a", alias = "active"))]
    pub active: usize,
    /// Everything that is one of a handful of choices, one character each:
    /// the slider's ordering, absolute or cumulative, the classifier's options
    /// as flags, and the colour the palette was holding. Spelled out, these
    /// four cost seventy characters of a link between them.
    #[cfg_attr(feature = "serde", serde(rename = "f", alias = "flags"))]
    pub flags: String,

    /// What share of each colour the street filters let through, unpainted
    /// first, one character each in twentieths. Empty means every colour passes
    /// whole, which is the ordinary case and costs nothing to say.
    #[cfg_attr(
        feature = "serde",
        serde(default),
        cfg_attr(feature = "serde", serde(rename = "cs"))
    )]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub shares: String,
    /// Which seat the statistics panel was comparing against, if any. A shared
    /// link should show what the reader was looking at, and the second column
    /// is part of that.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "cm", alias = "compare"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub compare: Option<usize>,
    /// Which seat the per-hand equity views were measured against, if one was
    /// named rather than the whole field.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "vs", alias = "versus"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub versus: Option<usize>,
    /// Which groups of flops a pass over them was narrowed to, one character
    /// per axis. Empty - the ordinary case - means every flop.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "fg", alias = "flop_groups"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub flop_groups: String,
    /// The seats.
    #[cfg_attr(feature = "serde", serde(rename = "p", alias = "players"))]
    pub players: Vec<PlayerSnapshot>,
}

impl Snapshot {
    /// The current format version.
    pub const VERSION: u32 = 1;
}

/// One saved seat.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct PlayerSnapshot {
    /// The seat's name, which is worked out rather than carried: a hand is
    /// named after its cards, and a range after its place in the row.
    #[cfg_attr(feature = "serde", serde(default, skip_serializing))]
    pub name: String,
    /// The hand this seat was dealt, if it is a hand rather than a range.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "h", alias = "hand"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub hand: Option<String>,
    /// The range, in text notation.
    #[cfg_attr(feature = "serde", serde(rename = "n", alias = "range"))]
    pub range: String,
    /// How the seat is coloured, written straight onto the seat rather than in
    /// an object of its own: the wrapper cost more than what it wrapped.
    #[cfg_attr(feature = "serde", serde(default, flatten))]
    pub groups: GroupsSnapshot,
    /// Whether the reader has taken the painting over from the default.
    #[cfg_attr(
        feature = "serde",
        serde(
            default,
            rename = "pt",
            alias = "painted",
            skip_serializing_if = "core::ops::Not::not"
        )
    )]
    pub painted: bool,
    /// Each street's filter, where one was pressed. Trailing empty slots are
    /// left off: a filter nobody pressed reads back the same either way.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "st", alias = "streets"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Vec::is_empty"))]
    pub streets: Vec<Option<StreetSnapshot>>,
    /// The equity cut's share, if one was set. The slice itself is recomputed
    /// from the board and the ranges, which are saved anyway.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "cu", alias = "cut_share"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub cut_share: Option<f64>,
    /// The painting from before the cut, so dropping it still puts that back.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "cr", alias = "cut_restore"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub cut_restore: Option<GroupsSnapshot>,
}

/// One street's filter, saved.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct StreetSnapshot {
    /// The board length it was pressed at.
    #[cfg_attr(feature = "serde", serde(rename = "dl", alias = "dealt"))]
    pub dealt: usize,
    /// The hands it kept, once it stopped following the painting.
    #[cfg_attr(feature = "serde", serde(default))]
    #[cfg_attr(feature = "serde", serde(rename = "fz", alias = "frozen"))]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "Option::is_none"))]
    pub frozen: Option<String>,
}

/// A grouping, as its two layers.
///
/// Saved as categories plus exceptions rather than a colour per hand, because
/// that is the difference between "this category is blue" and "this category has
/// been picked over" - and the second is the gear.
#[derive(Clone, Debug, Default, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "camelCase"))]
pub struct GroupsSnapshot {
    /// Categories and their colours, in the order they were painted, two
    /// characters each: which statistic, then which colour.
    ///
    /// The order is what a colour means where categories overlap, so it has to
    /// survive. Spelling each one out as `["flushdraw","green"]` cost four
    /// hundred characters of a shared link for what is two dozen small numbers.
    #[cfg_attr(
        feature = "serde",
        serde(default),
        cfg_attr(feature = "serde", serde(rename = "c"))
    )]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub cats: String,
    /// Hands picked over by hand, three characters each: two for which hand,
    /// one for which colour. Usually empty.
    #[cfg_attr(
        feature = "serde",
        serde(default),
        cfg_attr(feature = "serde", serde(rename = "x"))
    )]
    #[cfg_attr(feature = "serde", serde(skip_serializing_if = "String::is_empty"))]
    pub picks: String,
}

/// The alphabet small numbers are written in: URL-safe, one character each.
const DIGITS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";

/// One small number as one character.
fn to_digit(value: usize) -> char {
    char::from(DIGITS[value.min(63)])
}

/// And back, or `None` for anything not in the alphabet.
fn from_digit(c: char) -> Option<usize> {
    DIGITS.iter().position(|d| char::from(*d) == c)
}

/// Drops the empty slots off the end of a list.
///
/// Three street slots where only the flop was pressed is one filter and two
/// nothings, and the two nothings read back as nothing whether they travel or
/// not.
fn trim_trailing_nothing<T>(mut slots: Vec<Option<T>>) -> Vec<Option<T>> {
    while matches!(slots.last(), Some(None)) {
        slots.pop();
    }
    slots
}

/// Writes a grouping out.
fn save_groups(groups: &GroupSet) -> GroupsSnapshot {
    let mut cats = String::new();
    for (stat, colour) in groups.categories() {
        cats.push(to_digit(stat.index() as usize));
        cats.push(to_digit(colour as usize));
    }
    let mut picks = String::new();
    for (combo, colour) in groups.exceptions() {
        let index = combo.index() as usize;
        picks.push(to_digit(index / 64));
        picks.push(to_digit(index % 64));
        picks.push(to_digit(colour as usize));
    }
    GroupsSnapshot { cats, picks }
}

/// Reads one back, ignoring anything this build does not recognise.
fn load_groups(saved: &GroupsSnapshot, stats: &ComboStats) -> GroupSet {
    let mut groups = GroupSet::new();

    // The short form this build writes, then the long one older builds wrote:
    // a link that was shared before this change still has to open.
    let mut cats = saved.cats.chars();
    while let (Some(stat), Some(colour)) = (cats.next(), cats.next()) {
        if let (Some(stat), Some(colour)) = (from_digit(stat), from_digit(colour)) {
            if let Some(stat) = StatId::from_index(stat as u8) {
                groups.paint_stat(stat, in_palette(colour), stats);
            }
        }
    }
    let mut picks = saved.picks.chars();
    while let (Some(high), Some(low), Some(colour)) = (picks.next(), picks.next(), picks.next()) {
        let (Some(high), Some(low), Some(colour)) =
            (from_digit(high), from_digit(low), from_digit(colour))
        else {
            continue;
        };
        let index = high * 64 + low;
        if index < NUM_COMBOS {
            groups.set(Combo::from_index(index as u16), in_palette(colour), stats);
        }
    }
    groups
}

/// A saved colour, or unpainted when the palette no longer runs that far.
///
/// The palette has been shortened, and a link written before that carries
/// colours nothing can now show or unpaint. Reading them as unpainted is the
/// one reading that leaves the reader somewhere they can work from.
fn in_palette(saved: usize) -> Colour {
    if saved <= crate::groups::COLOURS {
        saved as Colour
    } else {
        crate::groups::NONE
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cards::{Card, HandClass};
    use crate::equity;
    use crate::groups::{colour_from_key, colour_key};

    fn session_on(board: &str, range: &str) -> Session {
        let mut session = Session::new();
        session.set_board_text(board).unwrap();
        session.set_active_range_text(range).unwrap();
        session
    }

    #[test]
    fn suited_cells_name_their_suits_only_when_it_says_something() {
        let aks = HandClass::from_index(1).index() as usize;
        let ako = HandClass::from_ranks(12, 11, false).index() as usize;

        // All four suits held is no information, so nothing is named.
        let mut session = session_on("Qs 7s 2d", "AKs");
        assert!(session.class_suits()[aks].is_empty());
        // And an offsuit cell never names suits: twelve combinations over two
        // suits each is too much to read off a cell, and ambiguous besides.
        session.set_active_range_text("AKo").unwrap();
        assert!(session.class_suits()[ako].is_empty());

        // Three of the four, and the cell names them, strongest suit first.
        session.set_active_range_text("AsKs,AhKh,AdKd").unwrap();
        assert_eq!(session.class_suits()[aks], b"shd");

        // A filter narrows the cell the same way a hand does: only the spades
        // make the flushdraw here, so only the spades are left.
        session.set_active_range_text("AKs").unwrap();
        session.clear_groups();
        session.paint_stat(StatId::FLUSH_DRAW, DEFAULT_COLOUR);
        session.toggle_street_filter(0);
        assert_eq!(session.class_suits()[aks], b"s");
    }

    #[test]
    fn a_card_on_the_board_is_not_a_choice_the_range_made() {
        let aks = 1usize;
        // The king of hearts takes AhKh off the table. The other three are all
        // that can be held, so holding them is holding the whole cell.
        let mut session = session_on("Kh 8d 3c", "AKs");
        assert!(session.class_suits()[aks].is_empty());

        // Dropping one of the three is a choice, and reads as one.
        session.set_active_range_text("AsKs,AdKd").unwrap();
        assert_eq!(session.class_suits()[aks], b"sd");

        // Nothing held names nothing: an empty cell is already drawn as empty.
        session.set_active_range_text("QQ").unwrap();
        assert!(session.class_suits()[aks].is_empty());
    }

    #[test]
    fn a_cell_opens_into_its_combinations() {
        let mut session = session_on("Kh 8d 3c", "AKs");
        let class = HandClass::from_index(1);
        let combos = session.cell_combos(class, None);
        assert_eq!(combos.len(), 4);
        // The one the board has taken is marked, not hidden: the breakdown is
        // the shape of the hand, and the shape does not change.
        let hearts = combos
            .iter()
            .find(|c| c.combo.to_string() == "AhKh")
            .unwrap();
        assert!(hearts.dealt);
        assert!(combos.iter().filter(|c| c.dealt).count() == 1);
        assert!(combos.iter().all(|c| c.weight == 1.0));

        // Within a statistic, only the combinations that match are marked.
        session.set_board_text("Qs 7s 2d").unwrap();
        session.set_active_range_text("AKs").unwrap();
        let combos = session.cell_combos(class, Some(StatId::FLUSH_DRAW));
        let matching: Vec<String> = combos
            .iter()
            .filter(|c| c.matches)
            .map(|c| c.combo.to_string())
            .collect();
        assert_eq!(matching, vec!["AsKs".to_string()]);
    }

    #[test]
    fn a_new_session_is_empty_and_preflop() {
        let session = Session::new();
        assert!(session.board().is_empty());
        assert!(session.active().range.is_empty());
        assert_eq!(session.players().len(), 2);
        assert!(!session.filters_enabled());
        assert!(session.equity().is_none());
    }

    #[test]
    fn the_board_drives_the_cache() {
        let mut session = session_on("Kc Qh Jh", "22+");
        assert_eq!(session.stats().board().len(), 3);
        let before = session.breakdown().total_combos;
        session.push_board_card(Card::parse("Ts").unwrap());
        assert_eq!(session.board().len(), 4);
        let after = session.breakdown().total_combos;
        assert!(after <= before, "a fourth card cannot add combos");
        session.pop_board_card();
        assert_eq!(session.board().len(), 3);
    }

    #[test]
    fn board_cards_cannot_be_dealt_twice() {
        let mut session = session_on("Kc Qh Jh", "22+");
        assert!(!session.push_board_card(Card::parse("Kc").unwrap()));
        session.toggle_dead(Card::parse("Kc").unwrap());
        assert!(!session.dead().contains(Card::parse("Kc").unwrap()));
    }

    #[test]
    fn dead_cards_only_take_cards_out_of_the_deck() {
        let mut session = session_on("Kc Qh Jh", "22+,AQs+");
        session.set_active(1);
        session.set_active_range_text("KK").unwrap();
        session.set_active(0);
        let before = session.live_combos();

        // Two of them, which used to be read as a hand and is not any more: the
        // seats say what is being measured, and these two cards say only that
        // nobody can be holding them.
        session.toggle_dead(Card::parse("Ah").unwrap());
        session.toggle_dead(Card::parse("Ad").unwrap());
        assert!(session.live_combos() < before, "the aces are gone from it");

        // The readout is still the seats against each other, not a hand against
        // one of them.
        assert_eq!(session.equity_seats(), vec![0, 1]);
        let report = session.equity().expect("two ranges have equity");
        assert!(report.exact);
        assert_eq!(report.players.len(), 2);
    }

    #[test]
    fn the_overlap_of_two_statistics_can_be_painted_in_one_go() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        session.clear_groups();

        // Ace high *and* a flushdraw - the hands worth continuing with that a
        // category ladder scatters across two rows.
        session.paint_intersection(StatId::FLUSH_DRAW, StatId::ACE_HIGH, 1);
        let both = Combo::parse("AhQh").unwrap();
        assert!(
            session.active().range.get(both) > 0.0,
            "AhQh is in the range"
        );
        assert!(session.stats().mask(both).has(StatId::FLUSH_DRAW));
        assert!(session.stats().mask(both).has(StatId::ACE_HIGH));
        assert_eq!(session.combo_colour(both), 1);

        // Either one on its own is left alone, which is the whole distinction.
        let ace_only = Combo::parse("AcQc").unwrap();
        assert!(session.stats().mask(ace_only).has(StatId::ACE_HIGH));
        assert!(!session.stats().mask(ace_only).has(StatId::FLUSH_DRAW));
        assert_eq!(session.combo_colour(ace_only), 0);

        // And the marker on each row becomes a gear, because neither category
        // is now wholly one colour.
        assert_eq!(session.mark(StatId::ACE_HIGH).key(), "mixed");
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "mixed");
    }

    #[test]
    fn the_statistics_can_be_taken_out_as_text() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");

        let text = session.statistics_text(None);
        assert!(text.starts_with("Kh 7h 2c\t"), "{text}");
        assert!(text.contains("top pair\t"));
        assert!(text.lines().count() > 10);

        // Hovering a row narrows the panel, and the text follows what is shown.
        let within = session.statistics_text(Some(StatId::FLUSH_DRAW));
        assert_ne!(within, text);

        // And the combos behind it, which is the other half of the question.
        let combos = session.statistics_combos(Some(StatId::FLUSH_DRAW));
        assert!(!combos.is_empty());
        for combo in combos.split(',') {
            let combo = Combo::parse(combo).unwrap();
            assert!(
                session.stats().mask(combo).has(StatId::FLUSH_DRAW),
                "{combo}"
            );
        }

        // What is copied is what is on screen, so a street filter cuts it too.
        let before = session.statistics_combos(None).split(',').count();
        session.paint_stat(StatId::TOP_PAIR, 1);
        session.toggle_street_filter(0);
        let after = session.statistics_combos(None).split(',').count();
        assert!(after < before, "{after} should be under {before}");
    }

    #[test]
    fn the_default_grouping_shows_no_gears_because_nobody_edited_anything() {
        let session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");

        // Categories overlap - a middle pair that is also a flushdraw gets
        // painted by the flushdraw rule - so "not every hand agrees" is true of
        // several of these. None of it was the reader's doing, so none of it is
        // a gear.
        for stat in StatId::all() {
            assert_ne!(
                session.mark(stat).key(),
                "mixed",
                "{stat:?} shows a gear on an untouched range"
            );
        }
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");
        assert_eq!(session.mark(StatId::SECOND_PAIR).key(), "none");
    }

    #[test]
    fn a_gear_appears_when_a_category_is_picked_over_by_hand_and_goes_when_it_is_repainted() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "blue");

        // One hand out of the category, chosen by hand.
        let one = session
            .stats()
            .live()
            .iter()
            .find(|combo| {
                session.active().range.get(*combo) > 0.0
                    && session.stats().mask(*combo).has(StatId::FLUSH_DRAW)
            })
            .unwrap();
        session.paint_combo(one, 2);
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "mixed");

        // Painting the whole category says what all of it should be, so the gear
        // has nothing left to warn about.
        session.paint_stat(StatId::FLUSH_DRAW, 1);
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "blue");

        // Clearing puts everything back to plain markers.
        session.paint_combo(one, 3);
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "mixed");
        session.clear_groups();
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "none");
    }

    #[test]
    fn the_slider_has_a_stop_wherever_the_equity_changes() {
        let mut session = session_on("Kh 7d 2c", "AA,KK,QQ,JJ");
        session.set_active(1);
        session.set_active_range_text("A2s+").unwrap();
        session.set_active(0);

        let steps = session.equity_steps();
        assert!(
            !steps.is_empty(),
            "a board and something to measure against"
        );
        // Ascending, ending at the whole range: a stop at nought would paint
        // nothing and is not a stop.
        assert!(steps.windows(2).all(|pair| pair[0] < pair[1]), "{steps:?}");
        assert!(steps[0] > 0.0);
        assert!(
            (steps[steps.len() - 1] - 1.0).abs() < 1e-6,
            "{:?}",
            steps.last()
        );

        // Four pairs, and every combination of a pair is worth what the others
        // are, so there are nothing like twenty-four stops.
        assert!(steps.len() <= 8, "{} stops for four pairs", steps.len());

        // Asking for exactly a step gets exactly that step, which is what lets
        // the slider show the reader the number it is going to act on.
        let at = f64::from(steps[0]);
        let next = f64::from(steps[1]);
        let on_the_step = session.set_continue_by_equity(at).expect("a cut").covered;
        assert!((on_the_step - at).abs() < 1e-6, "{on_the_step} for {at}");

        // Anywhere inside a gap paints the same thing - the step above it,
        // because a slice has to cover what was asked for. That is what makes
        // the places between steps worth skipping: they are all one place.
        let early = session
            .set_continue_by_equity(at + (next - at) * 0.25)
            .expect("a cut")
            .covered;
        let late = session
            .set_continue_by_equity(at + (next - at) * 0.75)
            .expect("a cut")
            .covered;
        assert_eq!(early, late, "the whole gap is one answer");
        assert!(
            (early - next).abs() < 1e-6,
            "and the answer is the step above"
        );

        // Before a flop there is nothing to sort by and the slider says so.
        let empty = Session::new();
        assert!(empty.equity_steps().is_empty());
    }

    #[test]
    fn a_dealt_hand_blocks_everything_that_counts_cards() {
        let mut session = session_on("Kh 7d 2c", "22+,A2s+");
        session.set_active(1);
        session.set_active_range_text("QQ+").unwrap();
        session.set_active(0);

        let before = Counts {
            flops: session.flop_breakdown().total,
            filter: session.flop_filter_count(),
            painted: session.painted_combos(),
            pass: session.preflop().flops,
            playable: session
                .cell_combos(HandClass::parse("AKs").unwrap(), None)
                .iter()
                .filter(|combo| !combo.dealt)
                .count(),
        };
        assert_eq!(before.flops, 22_100);

        // Two cards dealt to a seat of its own. They are gone from the deck:
        // no flop can hold them, no hand of anybody's range can use them, and
        // nothing that counts cards may go on pretending otherwise.
        session
            .add_hand(Combo::parse("AsKs").unwrap())
            .expect("room");
        let after = Counts {
            flops: session.flop_breakdown().total,
            filter: session.flop_filter_count(),
            painted: session.painted_combos(),
            pass: session.preflop().flops,
            playable: session
                .cell_combos(HandClass::parse("AKs").unwrap(), None)
                .iter()
                .filter(|combo| !combo.dealt)
                .count(),
        };

        // Two cards out of forty-seven leaves the flops that used either.
        assert!(
            after.flops < before.flops,
            "{} vs {}",
            after.flops,
            before.flops
        );
        assert_eq!(after.filter, after.flops, "the filter counts the same deck");
        assert_eq!(after.pass, after.flops, "and so does the pass");
        assert!(
            after.painted < before.painted,
            "the hands using those cards are gone"
        );

        // The breakdown of a cell counts one combination fewer, and names the
        // one that went.
        assert_eq!(
            after.playable,
            before.playable - 1,
            "the ace-king of spades is gone"
        );
        let cell = session.cell_combos(HandClass::parse("AKs").unwrap(), None);
        let spades = cell
            .iter()
            .find(|combo| combo.combo == Combo::parse("AsKs").unwrap())
            .expect("the cell holds it");
        assert!(spades.dealt, "somebody is holding it");

        // And a flop dealt from the panel never uses them either.
        for _ in 0..25 {
            assert!(session.deal_flop());
            let board = session.board().to_string();
            assert!(!board.contains("As") && !board.contains("Ks"), "{board}");
        }
    }

    /// The numbers the blocking test watches, read the same way twice.
    struct Counts {
        flops: u64,
        filter: u64,
        painted: f64,
        pass: u64,
        playable: usize,
    }

    #[test]
    fn hotness_finds_the_hand_wherever_it_is_sitting() {
        let mut session = session_on("Kh 7d 2c", "22+,A2s+");
        assert!(session.hotness().is_none(), "no hand, nothing to say");

        // Dealt at a seat of its own, and the reader left on their range: the
        // view is about that hand all the same, because there is only one.
        let hand = Combo::parse("AhKs").unwrap();
        session.add_hand(hand).expect("room at the table");
        assert_eq!(session.active_index(), 0, "still on the range");
        assert!(session.lone_combo().is_none(), "which is not a hand");
        let cards = session.hotness().expect("the hand at the table");
        assert_eq!(cards.len(), 52 - 3 - 2, "every card still to come");

        // Going to the hand asks the same question and gets the same answer.
        session.set_active(2);
        assert_eq!(
            session.hotness().map(|cards| cards.len()),
            Some(cards.len())
        );

        // The hand is measured against the other seats, not against the seat
        // the reader happens to be looking at - which would put the hand in its
        // own opposition, a matchup that cannot be dealt, and every card would
        // come out worth exactly nothing.
        session.set_active(0);
        let from_the_range = session.hotness().expect("the hand at the table");
        assert!(
            from_the_range.iter().any(|card| card.equity.abs() > 1e-6),
            "every card came out at nought, so the hand was facing itself"
        );

        // Two hands is a question with no answer: which of them?
        session
            .add_hand(Combo::parse("QdQs").unwrap())
            .expect("room");
        assert!(
            session.hotness().is_none(),
            "two hands, and nobody said which"
        );
    }

    #[test]
    fn a_cell_says_which_statistics_it_is_about() {
        let session = session_on("Kh 7d 2c", "22+");
        let index =
            |key: &str| crate::stats::stat_by_key(key).expect("a statistic").index() as usize;

        // A pair of aces on a king-high board is an overpair, every time.
        let aces = session.class_stats(HandClass::parse("AA").unwrap());
        assert_eq!(aces[index("overpair")], 1.0);
        assert_eq!(aces[index("top-pair")], 0.0);

        // A pair of kings is trips whenever it is not blocked - and the board
        // holds one king, so three of the six combinations are gone.
        let kings = session.class_stats(HandClass::parse("KK").unwrap());
        assert_eq!(kings[index("set")], 1.0);

        // A king in hand is top pair whichever king it is; the other card
        // decides nothing here.
        let king_queen = session.class_stats(HandClass::parse("KQo").unwrap());
        assert_eq!(king_queen[index("top-pair")], 1.0);

        // A cell that is only sometimes about a statistic says how often: of
        // the four suited aces, the one in the board's suit has two cards to a
        // backdoor flush and the other three have not.
        let ace_five = session.class_stats(HandClass::parse("A5s").unwrap());
        let backdoor = ace_five[index("bdfd-2")];
        assert!(backdoor > 0.0 && backdoor < 1.0, "{backdoor}");

        // One combination is a yes or a no, never a share.
        let one = session.combo_stats(Combo::parse("AsAh").unwrap());
        assert_eq!(one[index("overpair")], 1.0);
        assert!(one.iter().all(|share| *share == 0.0 || *share == 1.0));

        // A cell the board has taken away entirely has nothing to say.
        let seven_two = session.class_stats(HandClass::parse("77").unwrap());
        assert!(!seven_two.is_empty());
        let blocked = session_on("Kh Ks Kd", "22+").class_stats(HandClass::parse("KK").unwrap());
        assert!(blocked.is_empty(), "every king is on the board");
    }

    #[test]
    fn a_hand_is_made_deliberately_and_is_not_a_small_range() {
        let mut session = session_on("Kh 7d 2c", "22+,A2s+");

        // Typing one combination into a range is still a range: nobody has seen
        // those cards, so they are not out of the deck and it stays editable.
        session.set_active(1);
        session.set_active_range_text("AhQh").unwrap();
        assert!(session.lone_combo().is_none(), "a narrow range is a range");
        assert!(session.editable());
        session.set_active(0);

        // A hand is made in one action and named after its cards - but the
        // reader stays on the range they were working on, because a seat with
        // nothing to edit is not somewhere to be put without asking.
        let hand = Combo::parse("AsKs").unwrap();
        let seat = session.add_hand(hand).expect("room at the table");
        assert_eq!(
            session.active_index(),
            0,
            "dealing does not move the reader"
        );
        assert!(
            session.editable(),
            "and leaves them somewhere they can work"
        );
        assert_eq!(session.lone_combo_for(seat), Some(hand));
        assert_eq!(session.players()[seat].name, "AsKs");

        // There is nothing to do to it. Every way in refuses rather than half
        // working, because half of the seat would then disagree with the rest.
        session.set_active(seat);
        assert!(!session.editable());
        let before = session.active().range.to_notation();
        session.set_active_range_text("22+").unwrap();
        session.clear_active_range();
        session.paint_stat(StatId::TOP_PAIR, DEFAULT_COLOUR);
        assert!(!session.toggle_street_filter(0));
        assert_eq!(session.active().range.to_notation(), before);

        // And the cards it holds are out of the deck for the seats that do not
        // hold them - which is the whole reason a hand is not a range.
        session.set_active(0);
        let narrowed = session.effective_range();
        assert!(
            narrowed.get(Combo::parse("AsKs").unwrap()) == 0.0,
            "nobody else can hold the hand itself"
        );
        assert!(
            narrowed.get(Combo::parse("AsQs").unwrap()) == 0.0,
            "nor anything else needing the ace of spades"
        );
        assert!(
            narrowed.get(Combo::parse("AhQh").unwrap()) > 0.0,
            "the other suits are untouched"
        );

        // The hand still holds itself, though.
        session.set_active(seat);
        assert_eq!(session.effective_range().combo_count(), 1.0);
    }

    #[test]
    fn a_link_from_when_the_palette_was_longer_still_opens() {
        // Written when there were seven colours: the palette itself is holding
        // the sixth, one category is painted the seventh, and one hand was
        // picked out in the sixth.
        let mut snapshot = Session::new().snapshot();
        snapshot.flags = "0006".to_owned();
        snapshot.players[0].range = "AA,KK".to_owned();
        snapshot.players[0].groups.cats = format!("{}{}", to_digit(0), to_digit(7));
        snapshot.players[0].groups.picks = format!("{}{}{}", to_digit(0), to_digit(0), to_digit(6));

        // It opens, and everything the palette can no longer show reads as
        // unpainted - which is somewhere the reader can work from, unlike a
        // colour with no swatch to take it off with.
        let restored = Session::restore(&snapshot).expect("an older link still opens");
        assert_eq!(
            colour_key(restored.colour()),
            "blue",
            "the palette holds a colour it has"
        );
        for stat in StatId::all() {
            assert_ne!(
                restored.mark(stat).key(),
                "mixed",
                "{stat:?} kept a colour the palette cannot show"
            );
        }
        assert_eq!(
            restored.group_shares()[1..].iter().sum::<f64>(),
            0.0,
            "nothing is painted"
        );
    }

    #[test]
    fn a_hand_comes_back_from_a_link_as_a_hand() {
        let mut session = session_on("Kh 7d 2c", "22+,A2s+");
        let hand = Combo::parse("AsKs").unwrap();
        let seat = session.add_hand(hand).unwrap();

        let mut restored = Session::restore(&session.snapshot()).unwrap();
        assert_eq!(restored.lone_combo_for(seat), Some(hand));
        assert_eq!(restored.players()[seat].name, "AsKs");
        // And it is still not editable, which is the part a range that merely
        // looks the same would get wrong.
        restored.set_active(seat);
        assert!(!restored.editable());
        // Its cards are still out of the deck for the others.
        assert_eq!(restored.dealt_hands(), hand.mask());
    }

    #[test]
    fn a_hand_cannot_be_dealt_twice() {
        let mut session = session_on("Kh 7d 2c", "22+");
        assert!(session.add_hand(Combo::parse("AsKs").unwrap()).is_some());
        // The ace of spades is taken, so anything else needing it is not dealt.
        assert!(session.add_hand(Combo::parse("AsQd").unwrap()).is_none());
        // Nor is a hand that needs a card off the board.
        assert!(session.add_hand(Combo::parse("KhQh").unwrap()).is_none());
        assert!(session.add_hand(Combo::parse("AdQd").unwrap()).is_some());
    }

    #[test]
    fn a_hand_can_always_be_taken_away_and_the_letters_close_up() {
        let mut session = session_on("Kh 7d 2c", "22+");
        let seat = session.add_hand(Combo::parse("AsKs").unwrap()).unwrap();
        assert_eq!(
            session
                .players()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Range A", "Range B", "AsKs"]
        );

        // Two ranges is the floor, so neither of those goes.
        assert!(!session.remove_seat(0));
        // A hand is not a range and does not count towards it.
        assert!(session.remove_seat(seat));
        assert_eq!(session.players().len(), 2);

        // And a hand in the middle leaves the letters in order behind it.
        let middle = session.add_hand(Combo::parse("AdQd").unwrap()).unwrap();
        session.add_seat();
        assert!(session.remove_seat(middle));
        assert_eq!(
            session
                .players()
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["Range A", "Range B", "Range C"]
        );
    }

    #[test]
    fn the_per_hand_views_can_be_pointed_at_any_one_seat() {
        let mut session = session_on("Kh 7d 2c", "AhAs");
        session.add_seat();
        session.set_active(1);
        session.set_active_range_text("QhQs").unwrap();
        session.set_active(2);
        session.set_active_range_text("7h6h").unwrap();
        session.set_active(1);

        // Queens against the aces is a long way from queens against a pair of
        // sevens, and with three in the pot there is no way to ask about the
        // second one unless the seat can be named. This is the per-hand view,
        // which is always one range against one other; the readout beside it is
        // the three-way pot and a different question.
        let queens = Combo::parse("QhQs").unwrap();
        let equity_of_queens = |session: &Session| {
            f64::from(session.equity_by_combo().unwrap().equity[queens.index() as usize])
        };

        session.set_versus_seat(Some(0));
        let against_aces = equity_of_queens(&session);
        assert!(against_aces < 0.2, "queens are a long way behind aces");

        session.set_versus_seat(Some(2));
        let against_sevens = equity_of_queens(&session);
        assert!(
            against_sevens > 0.5,
            "and ahead of a pair of sevens: {against_sevens}"
        );

        // Naming no seat is the whole table laid over one another, which sits
        // between the two.
        session.set_versus_seat(None);
        let against_field = equity_of_queens(&session);
        assert!(
            against_field < against_sevens && against_field > against_aces,
            "the field holds both: {against_field} between {against_aces} and {against_sevens}"
        );

        // A seat with nothing in it is not a choice, and neither is this one.
        session.set_active(0);
        session.set_versus_seat(Some(0));
        assert_eq!(session.versus_seat(), None, "a seat cannot face itself");
    }

    #[test]
    fn a_checkmark_re_reads_the_pass_rather_than_re_running_it() {
        let mut session = Session::new();
        session.set_active_range_text("AA,KK,QQ").unwrap();

        // Nothing counts as a hit yet, so nothing has hit.
        let first = session.preflop();
        assert_eq!(first.hit, 0.0);
        assert!(first.flops > 0 && first.total > 0.0);
        assert!(!first.profile.is_empty(), "the pass records what it saw");

        // Tick a statistic: the headline answers immediately, off the shape the
        // pass already has, and the per-row numbers do not move at all.
        session.toggle_checkmark(StatId::OVERPAIR);
        let hit = session.preflop_hit().expect("the pass still applies");
        assert!(hit > 0.0, "overpairs hit sometimes");
        let second = session.preflop();
        assert!((second.hit - hit).abs() < 1e-12);
        assert_eq!(
            second.rows, first.rows,
            "which statistics count as a hit does not change what the range does"
        );

        // A different range is a different pass, and the old answer is retired
        // rather than quietly re-used.
        session.set_active_range_text("72o").unwrap();
        assert!(session.preflop_hit().is_none());
    }

    #[test]
    fn a_pass_stays_with_its_seat_when_the_reader_moves_to_another() {
        let mut session = Session::new();
        session.set_active_range_text("AA,KK,QQ").unwrap();
        session.set_active(1);
        session.set_active_range_text("72o,83o").unwrap();

        // Run one on each seat. The second does not displace the first: the
        // whole reason to run two is to hold them side by side.
        session.set_active(0);
        let pairs = session.preflop();
        session.set_active(1);
        let rags = session.preflop();
        assert_ne!(pairs.rows, rags.rows, "two ranges, two answers");

        // Back to the first, and it is still there - the same answer, not a
        // second sitting of the same work.
        session.set_active(0);
        let again = session.preflop_cached().expect("the pass is still here");
        assert_eq!(again.rows, pairs.rows);
        assert_eq!(session.preflop_cached().map(|p| p.flops), Some(pairs.flops));

        // Changing what a seat holds retires that seat's pass and leaves the
        // other one alone.
        session.set_active_range_text("AA").unwrap();
        assert!(session.preflop_cached().is_none());
        session.set_active(1);
        assert!(
            session.preflop_cached().is_some(),
            "the other seat is untouched"
        );
    }

    #[test]
    fn narrowing_to_a_group_of_flops_asks_the_same_question_of_every_seat() {
        let mut session = Session::new();
        session.set_active_range_text("AKs").unwrap();
        session.set_active(1);
        session.set_active_range_text("22").unwrap();
        session.set_active(0);

        let everything = session.preflop();
        assert_eq!(everything.flops, 22_100);

        // Narrow to monotone flops. A pass over them is a different pass, so
        // the old answer no longer applies - and the new one is over fewer
        // flops.
        assert!(session.toggle_flop_group("suits", "monotone"));
        assert!(
            session.preflop_cached().is_none(),
            "a new question, not the old answer"
        );
        let monotone = session.preflop();
        assert_eq!(monotone.flops, session.flop_filter_count());
        assert!(monotone.flops < everything.flops);
        // A suited hand flops a flush far more often on three of a suit than
        // on all flops, which is the sort of thing the narrowing is for.
        let flushes = |pass: &PreflopBreakdown| pass.row("flush").map_or(0.0, |row| row.fraction);
        assert!(flushes(&monotone) > flushes(&everything) * 10.0);

        // The narrowing belongs to the table: moving to the other seat asks it
        // the same question rather than putting every flop back.
        session.set_active(1);
        assert_eq!(session.flop_filter_count(), monotone.flops);
        assert_eq!(session.preflop().flops, monotone.flops);

        // And it survives the round trip through a link, because a shared link
        // should show what the reader was looking at.
        let restored = Session::restore(&session.snapshot()).expect("a readable link");
        assert_eq!(restored.flop_filter_count(), monotone.flops);
        assert!(restored.flop_filter().contains("suits", "monotone"));

        // Untick it and every flop is back.
        session.clear_flop_filter();
        assert_eq!(session.preflop().flops, 22_100);
    }

    #[test]
    fn a_dealt_flop_comes_from_the_groups_that_are_ticked() {
        let mut session = Session::new();
        session.set_active_range_text("AKs").unwrap();

        // Nothing ticked: any flop at all, which is the plain deal.
        assert!(session.deal_flop());
        assert_eq!(session.board().cards().count(), 3);

        // Ticked: every deal lands inside the selection. Twenty of them,
        // because one could be luck.
        session.toggle_flop_group("suits", "monotone");
        session.toggle_flop_group("high-card", "A");
        for _ in 0..20 {
            assert!(session.deal_flop(), "the selection is not empty");
            let board = session.board().to_string();
            let cards: Vec<&str> = board.split_whitespace().collect();
            assert_eq!(cards.len(), 3, "{board}");
            let suits: std::collections::HashSet<char> = cards
                .iter()
                .filter_map(|card| card.chars().nth(1))
                .collect();
            assert_eq!(suits.len(), 1, "{board} is not monotone");
            assert!(
                cards.iter().any(|card| card.starts_with('A')),
                "{board} is not ace high"
            );
        }

        // A selection the dead cards have emptied deals nothing rather than
        // something outside it.
        session.clear_flop_filter();
        session.toggle_flop_group("suits", "monotone");
        session.set_board_text("").unwrap();
        session.set_dead(CardSet::parse("2s 3s 4s 5s 6s 7s 8s 9s Ts Js Qs Ks As").unwrap());
        session.toggle_flop_group("high-card", "A");
        // Every spade is gone and every remaining monotone flop is a heart,
        // diamond or club one - so ace-high monotone still exists. Take the
        // aces too and there is nothing left.
        session
            .set_dead(CardSet::parse("2s 3s 4s 5s 6s 7s 8s 9s Ts Js Qs Ks As Ah Ad Ac").unwrap());
        assert_eq!(session.flop_filter_count(), 0);
        assert!(!session.deal_flop(), "nothing to deal, and nothing dealt");
    }

    #[test]
    fn dead_cards_thin_the_narrowed_flops_too() {
        let mut session = Session::new();
        session.set_active_range_text("AKs").unwrap();
        session.toggle_flop_group("high-card", "A");
        let wide = session.flop_filter_count();

        // Holding two aces yourself takes ace-high flops off the table in a way
        // the panel's own count already knows about; the filter agrees with it.
        session.set_dead(CardSet::parse("As Ad").unwrap());
        let narrow = session.flop_filter_count();
        assert!(narrow < wide, "{narrow} should be under {wide}");
        assert_eq!(session.preflop().flops, narrow);
    }

    #[test]
    fn inverting_the_categories_flips_the_marks_and_nothing_else() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");
        assert_eq!(session.mark(StatId::SECOND_PAIR).key(), "none");

        session.invert_categories();

        // Every mark down the side of the panel turns over.
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "none");
        assert_eq!(session.mark(StatId::SECOND_PAIR).key(), "blue");
        // And nothing was picked over, so nothing grows a gear.
        for stat in StatId::all() {
            assert_ne!(session.mark(stat).key(), "mixed", "{stat:?} grew a gear");
        }

        // The hands do not come out as the complement, and cannot: a hand in one
        // painted and one unpainted category is painted on both sides of the
        // flip. That is the difference between this and inverting the hands.
        let shares = session.group_shares();
        let total: f64 = shares.iter().sum();
        session.invert_categories();
        let back: f64 = session.group_shares().iter().sum();
        assert!(
            (total - back).abs() < 1e-6,
            "the range does not change size"
        );
    }

    #[test]
    fn the_two_inversions_are_different_questions() {
        let mut categories = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        let mut hands = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        let before = categories.group_shares()[1];

        categories.invert_categories();
        hands.invert_groups();

        // Inverting the hands gives back exactly what was not painted.
        let by_hand = hands.group_shares();
        assert!((by_hand[1] - (by_hand.iter().sum::<f64>() - before)).abs() < 1e-6);
        // Inverting the categories gives back more than that, because the hands
        // in two categories at once are painted either way round.
        let by_category = categories.group_shares();
        assert!(
            by_category[1] > by_hand[1] + 1.0,
            "a category flip keeps the overlaps: {by_category:?} against {by_hand:?}"
        );
    }

    #[test]
    fn inverting_gives_back_what_was_not_painted() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");
        assert_eq!(session.mark(StatId::SECOND_PAIR).key(), "none");
        let before = session.group_shares();

        session.invert_groups();

        // The pot of paint changes hands: what was painted is bare, and what
        // was bare is painted, to the last combination. The two readings are
        // one range said twice, so they add up to it rather than past it.
        let after = session.group_shares();
        assert!(
            (after[1] - before[0]).abs() < 1e-6,
            "after {after:?} against before {before:?}"
        );
        assert!((after[0] - before[1]).abs() < 1e-6);

        // A category painted whole comes out bare.
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "none");

        // And twice over is where it started.
        session.invert_groups();
        let back = session.group_shares();
        for colour in 0..back.len() {
            assert!((back[colour] - before[colour]).abs() < 1e-6, "{back:?}");
        }
    }
    #[test]
    fn the_equity_slider_puts_gears_on_the_categories_it_splits() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        session.set_continue_by_equity(0.5);

        // It cuts across the categories rather than along them, so the ones it
        // splits say so - and the ones it took or left whole do not.
        let gears = StatId::all()
            .filter(|stat| session.mark(*stat).key() == "mixed")
            .count();
        assert!(gears > 0, "the slider split nothing");
        assert!(
            gears < StatId::all().count(),
            "it cannot have split everything"
        );

        // Undo restores both the painting and the markers.
        session.clear_cut();
        for stat in StatId::all() {
            assert_ne!(session.mark(stat).key(), "mixed", "{stat:?} kept its gear");
        }
    }

    #[test]
    fn gears_survive_a_snapshot() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        let one = Combo::parse("AhQh").unwrap();
        session.paint_combo(one, 2);
        let gears: Vec<&str> = StatId::all()
            .filter(|stat| session.mark(*stat).key() == "mixed")
            .map(|stat| stat.def().key)
            .collect();
        assert!(!gears.is_empty());

        let restored = Session::restore(&session.snapshot()).unwrap();
        let after: Vec<&str> = StatId::all()
            .filter(|stat| restored.mark(*stat).key() == "mixed")
            .map(|stat| stat.def().key)
            .collect();
        assert_eq!(after, gears);
    }

    #[test]
    fn clearing_sticks_across_a_card_and_lifts_on_a_new_range() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");

        session.clear_filters();
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "none");

        // A card landing does not undo what the reader just asked for.
        session.push_board_card(Card::parse("2s").unwrap());
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "none");

        // A different range is a different question, and leaving the reader with
        // a range the panel has no opinion about is worse than offering one.
        session.set_active_range_text("TT+,AQs+,AKo").unwrap();
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");

        // A grouping somebody painted survives a new range, because it is theirs.
        session.paint_stat(StatId::TOP_PAIR, 3);
        session.set_active_range_text("22+,A2s+").unwrap();
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "red");
    }

    #[test]
    fn a_seat_nobody_has_painted_opens_on_the_default_grouping() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");
        assert_eq!(session.mark(StatId::FLUSH_DRAW).key(), "blue");
        assert_ne!(session.mark(StatId::SECOND_PAIR).key(), "blue");

        // A new card re-reads the default, because it is still the default.
        session.push_board_card(Card::parse("2s").unwrap());
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");

        // Once anything is said about the painting, it stops being the default's
        // to change - including saying there should be nothing painted.
        session.clear_groups();
        session.push_board_card(Card::parse("3c").unwrap());
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "none");

        session.reset_groups();
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");
    }

    #[test]
    fn painting_alone_changes_nothing_until_a_street_is_applied() {
        let mut session = session_on("Kc Qh Jh", "22+,A2s+,KJs+,AJo+");
        session.clear_groups();
        let wide = session.breakdown().total_combos;

        // Painting says which hands are which. It does not remove any of them.
        session.paint_stat(StatId::TOP_PAIR, 1);
        assert_eq!(
            session.breakdown().total_combos,
            wide,
            "painting narrows nothing"
        );
        assert!(!session.filters_enabled());
        assert_eq!(session.mark(StatId::TOP_PAIR).key(), "blue");

        // Pressing the flop's filter is what narrows it.
        assert!(session.toggle_street_filter(0));
        let narrow = session.breakdown().total_combos;
        assert!(narrow < wide, "{narrow} should be under {wide}");
        assert!(session.filters_enabled());
        assert!((session.pass_fraction() - narrow / wide).abs() < 1e-9);
        assert!(session.street_applied(0));
        assert!((session.street_count(0) - narrow).abs() < 1e-9);

        // And pressing it again takes it back.
        assert!(!session.toggle_street_filter(0));
        assert_eq!(session.breakdown().total_combos, wide);
        assert!(!session.street_applied(0));
    }

    #[test]
    fn a_live_filter_follows_the_painting_until_a_card_lands() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        session.clear_groups();
        session.paint_stat(StatId::TOP_PAIR, 1);
        session.toggle_street_filter(0);
        let with_top_pair = session.street_count(0);
        assert!(with_top_pair > 0.0);

        // Still on the flop, so painting more keeps more - the count moves with
        // the marks, which is what "the filters are on" has to mean.
        session.paint_stat(StatId::FLUSH_DRAW, 1);
        let with_draws = session.street_count(0);
        assert!(
            with_draws > with_top_pair,
            "{with_draws} should be over {with_top_pair}"
        );
        assert!((session.effective_range().combo_count() - with_draws).abs() < 1e-9);

        // Unpainting takes them back out again.
        session.paint_stat(StatId::FLUSH_DRAW, crate::groups::NONE);
        assert!((session.street_count(0) - with_top_pair).abs() < 1e-9);

        // A card lands, and what continued on the flop becomes a fact about the
        // flop: repainting now cannot reach back and change it.
        session.paint_stat(StatId::FLUSH_DRAW, 1);
        let at_the_flop = session.street_count(0);
        session.push_board_card(Card::parse("2s").unwrap());
        assert!((session.street_count(0) - at_the_flop).abs() < 1e-9);
        session.clear_groups();
        assert!((session.street_count(0) - at_the_flop).abs() < 1e-9);
    }

    #[test]
    fn the_street_counts_read_down_as_a_chain_and_end_where_the_footer_does() {
        let mut session = session_on("Qs 2h 7s", "22+,AQs+,AJo+,KQo");
        session.clear_groups();
        session.paint_stat(StatId::TOP_PAIR, 1);
        session.paint_stat(StatId::SET, 1);
        session.paint_stat(StatId::TWO_PAIR, 1);
        session.toggle_street_filter(0);

        let live = session.live_combos();
        let after_flop = session.street_count(0);
        assert!(after_flop > 0.0 && after_flop < live);
        // The button and the footer are the same number, because they are the
        // same fact said twice.
        assert!((live * session.pass_fraction() - after_flop).abs() < 1e-9);

        // A card lands. Every number is counted against the board as it is now,
        // so the flop's own count falls by whatever the turn card blocked.
        session.push_board_card(Card::parse("Kh").unwrap());
        let after_card = session.street_count(0);
        assert!(after_card <= after_flop);
        assert!((session.live_combos() * session.pass_fraction() - after_card).abs() < 1e-9);

        // The turn's button, before it is pressed, says what pressing it would
        // leave - which can only be less than what came into the turn.
        session.clear_groups();
        session.paint_stat(StatId::TOP_PAIR, 1);
        let would = session.street_count(1);
        assert!(would <= after_card, "{would} cannot exceed {after_card}");

        // Pressing it changes nothing about that number, and the footer follows.
        session.toggle_street_filter(1);
        assert!((session.street_count(1) - would).abs() < 1e-9);
        assert!((session.live_combos() * session.pass_fraction() - would).abs() < 1e-9);
        // And the chain still reads down.
        assert!(session.street_count(0) >= session.street_count(1));
    }

    #[test]
    fn each_street_narrows_what_the_one_before_it_left() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        session.clear_groups();
        session.paint_stat(StatId::TOP_PAIR, 1);
        session.paint_stat(StatId::FLUSH_DRAW, 1);
        session.toggle_street_filter(0);
        let after_flop = session.effective_range().combo_count();

        session.push_board_card(Card::parse("2s").unwrap());
        assert_eq!(session.streets_dealt(), 2);

        // Repaint for the turn: only the made hands go on.
        session.clear_groups();
        session.paint_stat(StatId::TOP_PAIR, 1);
        session.toggle_street_filter(1);
        let after_turn = session.effective_range().combo_count();
        assert!(
            after_turn < after_flop,
            "the turn should cut into {after_flop}, got {after_turn}"
        );

        // The flop's set is still doing its job: lifting the turn does not give
        // back the hands the flop had already removed.
        session.toggle_street_filter(1);
        assert!(session.effective_range().combo_count() <= after_flop);
    }

    #[test]
    fn seats_are_independent() {
        let mut session = session_on("Kc Qh Jh", "AA");
        session.set_active(1);
        assert!(session.active().range.is_empty());
        session.set_active_range_text("KK").unwrap();
        session.set_active(0);
        assert_eq!(session.active().range.to_notation(), "AA");
        session.set_active(1);
        assert_eq!(session.active().range.to_notation(), "KK");
    }

    #[test]
    fn range_versus_range_uses_the_first_two_seats() {
        let mut session = session_on("Kc 7d 2s", "AA");
        session.set_active(1);
        session.set_active_range_text("KQs").unwrap();
        let report = session.equity().expect("two ranges have equity");
        let sum = report.players[0].equity + report.players[1].equity;
        assert!((sum - 1.0).abs() < 1e-6);
    }

    #[test]
    fn the_slider_and_the_matrix_agree() {
        let mut session = Session::new();
        session.set_active_top_percent(10.0);
        let count = session.active().range.combo_count();
        assert!(count > 0.0 && count <= 132.6);
        session.set_active_class(HandClass::parse("72o").unwrap(), 1.0);
        assert!(session.active().range.combo_count() > count);
        session.clear_active_range();
        assert!(session.active().range.is_empty());
    }

    #[test]
    fn the_handles_park_on_the_chart_they_describe() {
        // Loading a chart does not leave the handles where the last range put
        // them: they move to the window that comes closest to this one, which
        // says how wide it is and leaves both of them somewhere to go.
        let mut session = Session::new();
        assert!(session.load_library("cash-nl10-defend-utg"));
        let chart = session.active().range.clone();
        let parked = session.slider();
        assert_eq!(parked.low, 0.0, "the chart holds the best hands there are");
        let width = chart.percent_of_deck() * 100.0;
        assert!(
            (parked.high - width).abs() < 6.0,
            "parked at {} for a {width:.1}% chart",
            parked.high
        );
        // Both handles have somewhere to go, which is the whole point: right to
        // take in the hands the chart folds, left to cut off the ones it
        // three-bets.
        assert!(parked.high < 100.0);
        assert_eq!(session.active().range, chart, "parking moved no hands");

        // The chart is still named after an edit, and known to be edited.
        assert!(!session.library_edited());
        assert_eq!(
            session.from_library().map(|from| from.id.as_str()),
            Some("cash-nl10-defend-utg")
        );
        session.set_active_class(HandClass::parse("72o").unwrap(), 1.0);
        assert!(session.library_edited());
        assert_eq!(
            session.from_library().map(|from| from.id.as_str()),
            Some("cash-nl10-defend-utg"),
            "edited, not forgotten"
        );

        // Clearing is starting again rather than editing.
        session.clear_active_range();
        assert!(session.from_library().is_none());
        assert!(!session.library_edited());
    }

    #[test]
    fn the_slider_reaches_the_whole_deck_whatever_is_in_the_matrix() {
        // The complaint this answers: with the handles spanning the chart
        // instead of the deck, the blue one was already at its right-hand stop
        // and there was no way to take in a hand the chart folds.
        let mut session = Session::new();
        assert!(session.load_library("cash-nl10-open-utg"));
        let opens = session.active().range.combo_count();
        session.set_active_window(0.0, 100.0);
        // Every cell, and the chart's own cells at the weights it gave them -
        // widening a chart must not quietly turn its mixed cells into pure ones.
        assert_eq!(
            session.active().range.cell_percent(),
            100.0,
            "the far right leaves no cell out"
        );
        assert_eq!(
            session.active().range.get(Combo::parse("7h2d").unwrap()),
            1.0,
            "a hand the chart folds arrives whole"
        );
        assert!(session.active().range.combo_count() > opens);
        assert!(session.library_edited(), "which is not the chart any more");

        assert!(session.load_library("cash-nl10-open-utg"));
        let (low, high) = (session.slider().low, session.slider().high);
        session.set_active_window(low, high + 10.0);
        assert!(
            session.active().range.combo_count() > opens,
            "widening adds hands the chart folded"
        );

        // And at rest it is the chart, untouched: parking is not a cut.
        assert!(session.load_library("cash-nl10-open-utg"));
        let chart = session.active().range.clone();
        let (low, high) = (session.slider().low, session.slider().high);
        session.set_active_window(low, high);
        assert_eq!(
            session.active().range,
            chart,
            "the handles were already here"
        );
    }

    #[test]
    fn an_empty_matrix_leaves_the_slider_closed() {
        // With nothing to describe the handles sit together at the top, and
        // opening one builds a range out of the ranking, strongest hands first.
        let mut session = Session::new();
        session.clear_active_range();
        assert_eq!(session.slider().low, 0.0);
        assert_eq!(session.slider().high, 0.0);

        session.set_active_window(0.0, 10.0);
        let built = session.active().range.combo_count();
        assert!(built > 0.0 && built <= 0.10 * 1326.0);
        assert_eq!(
            session.active().range,
            Range::window(0.0, 10.0, session.ranking()),
            "the same band it has always selected"
        );
    }

    #[test]
    fn preflop_equity_comes_with_the_pass_over_the_flops() {
        let mut session = Session::new();
        session.set_active_range_text("22+, A2s+, ATo+").unwrap();
        session.add_player("Range B");
        session.set_active(1);
        session
            .set_active_range_text("22+, A2s+, K9s+, A8o+")
            .unwrap();
        session.set_active(0);

        // Nothing until the pass is run: every redraw reads this, and none of
        // them may cost a second.
        assert!(session.preflop_equity_work() > 0.0);
        assert!(!session.preflop_equity_ready());
        assert!(session.equity_by_combo().is_none());

        // One pass, both answers - there is no second button to press.
        session.preflop();
        assert!(session.preflop_equity_ready());
        let got = session.equity_by_combo().expect("a pass was run");

        // Aces beat sevens beat the ace-ten, and every hand in the range has a
        // number while the ones outside it have none.
        let of = |hand: &str| got.equity[Combo::parse(hand).unwrap().index() as usize];
        assert!(of("AhAs") > of("7h7s"), "{} vs {}", of("AhAs"), of("7h7s"));
        assert!(of("7h7s") > of("AhTd"));
        assert!(of("AhAs") > 0.7 && of("AhAs") < 0.95);
        assert_eq!(of("7h2d"), -1.0, "not in the range");

        // Both curves, because the graph draws both.
        assert!(session.opponent_equity_by_combo().is_some());

        // And it is about the ranges it was run on: move one and the pass is
        // gone rather than quietly wrong.
        session.set_active_class(HandClass::parse("72o").unwrap(), 1.0);
        assert!(!session.preflop_equity_ready());
        assert!(session.equity_by_combo().is_none());
    }

    #[test]
    fn one_pass_answers_both_seats() {
        // The graph draws both curves, so a pass works out both directions -
        // and the other seat's question is this one backwards. Filing the
        // answer by the pair rather than by the seat is what stops the reader
        // being asked for a second run of work already done.
        let mut session = Session::new();
        session.set_active_range_text("22+, AQs+, AKo").unwrap();
        session.set_active(1);
        session
            .set_active_range_text("22+, A2s+, K9s+, A8o+")
            .unwrap();
        session.set_active(0);

        session.preflop();
        let from_a = session.equity_by_combo().expect("the seat that ran it");
        let theirs = session
            .opponent_equity_by_combo()
            .expect("and the other curve");

        session.set_active(1);
        let from_b = session
            .equity_by_combo()
            .expect("the other seat, with nothing else pressed");
        assert_eq!(from_b, theirs, "which is the curve A already drew");
        assert!(session.preflop_equity_ready());
        assert_eq!(
            session.opponent_equity_by_combo(),
            Some(from_a),
            "and looking back the other way is the first curve again"
        );
    }

    #[test]
    fn a_dealt_hand_is_something_to_measure_against() {
        // A hand dealt at the other seat is the range on that side. Crossing
        // its own two cards out of it - which is what blocking by every hand
        // at the table did - left nothing to measure, and the equity views
        // went blank unless the reader looked from the hand's own seat.
        let mut session = Session::new();
        session.set_active_range_text("22+, AQs+, AKo").unwrap();
        let seat = session
            .add_hand(Combo::parse("KhKs").unwrap())
            .expect("room at the table");
        // The empty second seat is not in the pot; the hand is.
        session.set_active(1);
        session.clear_active_range();
        session.set_active(0);

        session.preflop();
        let got = session.equity_by_combo().expect("a range against a hand");
        let of = |hand: &str| f64::from(got.equity[Combo::parse(hand).unwrap().index() as usize]);
        // Aces are ahead of a pair of kings; the other pairs are well behind.
        assert!(of("AhAs") > 0.7, "{:.4}", of("AhAs"));
        assert!(of("7h7s") < 0.3, "{:.4}", of("7h7s"));
        // The hand blocks the two kings it holds, so they are not in the range
        // any more - but the two that are left are still measured.
        assert_eq!(of("KhKd"), -1.0, "the hand holds that king");
        assert!(of("KcKd") > 0.0);

        // And from the hand's own seat the question is the same one backwards,
        // which the pass has already answered.
        session.set_active(seat);
        assert!(session.preflop_equity_ready());
        let hand = session
            .equity_by_combo()
            .expect("the hand against the range");
        assert!(hand.equity[Combo::parse("KhKs").unwrap().index() as usize] > 0.4);
    }

    #[test]
    fn preflop_equity_is_over_the_flops_the_ticks_leave() {
        // Asking what a range is worth on paired boards is a different question
        // from asking what it is worth anywhere, and the panel that asks one
        // must not quietly answer the other.
        let mut session = Session::new();
        session.set_active_range_text("77, AKo").unwrap();
        session.add_player("Range B");
        session.set_active(1);
        session
            .set_active_range_text("22+, A2s+, K9s+, A8o+")
            .unwrap();
        session.set_active(0);

        session.preflop();
        let anywhere = session.equity_by_combo().expect("a pass was run");

        // Ticking a group is a different question, so the answer goes with it.
        assert!(session.toggle_flop_group("pairing", "paired-top"));
        assert!(!session.preflop_equity_ready());

        session.preflop();
        let paired = session.equity_by_combo().expect("and another pass");
        let of = |data: &ComboEquity, hand: &str| {
            f64::from(data.equity[Combo::parse(hand).unwrap().index() as usize])
        };
        // A board paired at the top leaves ace-king with one pair to make and
        // hands the field trips, so it is worth less there than anywhere.
        assert!(
            of(&paired, "AhKd") < of(&anywhere, "AhKd") - 0.01,
            "{:.4} on top-paired boards against {:.4} anywhere",
            of(&paired, "AhKd"),
            of(&anywhere, "AhKd")
        );

        // Clearing the ticks asks the wider question again, and gets the same
        // answer as before - the sampler is seeded, so this is reproducible.
        session.clear_flop_filter();
        session.preflop();
        let back = session.equity_by_combo().expect("and a third");
        assert!((of(&back, "AhKd") - of(&anywhere, "AhKd")).abs() < 1e-9);
    }

    #[test]
    fn preflop_equity_agrees_with_the_hand_by_hand_estimate() {
        let mut session = Session::new();
        session
            .set_active_range_text("22+, A2s+, K9s+, QTs+, ATo+")
            .unwrap();
        session.add_player("Range B");
        session.set_active(1);
        let villain = Range::parse("22+, A2s+, K2s+, Q8s+, J9s+, A2o+, K9o+").unwrap();
        session
            .set_active_range_text(&villain.to_notation())
            .unwrap();
        session.set_active(0);
        session.preflop();
        let got = session.equity_by_combo().unwrap();

        // Against the one-hand-at-a-time sampler, which draws its own hands and
        // its own boards and so agrees only if both are right.
        for hand in ["AhAs", "KhKs", "AhKh", "7h7s", "QhTh", "AhJd"] {
            let combo = Combo::parse(hand).unwrap();
            let mine = f64::from(got.equity[combo.index() as usize]);
            let check = equity::hand_vs_range(combo, &villain, &Board::empty(), CardSet::EMPTY)
                .expect("a hand against a range")
                .players[0]
                .equity;
            assert!(
                (mine - check).abs() < 0.02,
                "{hand}: {mine:.4} against {check:.4}"
            );
        }
    }

    #[test]
    fn the_range_splits_into_four_bands_of_equity() {
        // The other question the panel answers: not what a hand is, but what
        // it is worth. A set and a gutshot are rungs apart on the ladder and
        // the bands put them where they belong.
        let mut session = session_on("Kh 7d 2c", "22+, A2s+, K9s+, A8o+");
        session.set_active(1);
        session
            .set_active_range_text("22+, A2s+, K2s+, Q8s+, J9s+, A2o+, K9o+")
            .unwrap();
        session.set_active(0);

        let bands = session.equity_buckets();
        assert_eq!(
            bands.iter().map(|band| band.label).collect::<Vec<_>>(),
            ["best hands", "good hands", "weak hands", "trash hands"]
        );
        // They are the whole range and nothing twice.
        let whole: f64 = bands.iter().map(|band| band.fraction).sum();
        assert!((whole - 1.0).abs() < 1e-9, "{whole}");
        assert!(bands.iter().all(|band| band.fraction >= 0.0));
        assert!(bands.iter().any(|band| band.fraction > 0.0));

        // A king on the board, so a range holding kings has something in the
        // top band and something in the bottom one.
        assert!(bands[0].fraction > 0.0, "{:?}", bands[0]);
        assert!(bands[3].fraction > 0.0, "{:?}", bands[3]);

        // The bands are quarters, which is what makes them readable without a
        // legend - and they meet without a gap or an overlap.
        for (band, (low, high)) in bands.iter().zip([(75, 100), (50, 75), (25, 50), (0, 25)]) {
            assert_eq!((band.low, band.high), (low, high), "{}", band.label);
        }
    }

    #[test]
    fn there_are_no_bands_without_something_to_measure_against() {
        // One range and no board: nothing to have equity against, so the block
        // has nothing to say and says nothing rather than saying nought.
        let mut session = Session::new();
        session.set_active_range_text("22+, A2s+").unwrap();
        assert!(session.equity_buckets().is_empty());

        // A board on its own does not help; it takes two ranges.
        session.set_board_text("Kh 7d 2c").unwrap();
        assert!(session.equity_buckets().is_empty());
    }

    #[test]
    fn painting_part_of_a_category_takes_it_by_equity() {
        // The spot this is for: a reader building a betting range wants the
        // best of the rubbish, and "trash hands" on its own cannot say which
        // those are.
        let mut session = session_on("Kh 7d 2c", "22+, A2s+, K9s+, A8o+, KJo+");
        session.set_active(1);
        session
            .set_active_range_text("22+, A2s+, K2s+, Q8s+, J9s+, A2o+, K9o+")
            .unwrap();
        session.set_active(0);

        let top_pair = crate::stats::stat_by_key("top-pair").unwrap();
        let whole = session
            .breakdown()
            .row(top_pair)
            .map(|row| row.combos)
            .unwrap();
        assert!(whole > 0.0);

        // Starting from nothing painted, or the default grouping is counted in
        // as well and every measurement below is of the wrong set.
        let blue = crate::groups::colour_from_key("blue").unwrap();
        session.clear_groups();

        // A fifth of it, and the fifth that is worth the most.
        assert!(session.paint_stat_part(top_pair, 0.0, 20.0, blue));
        let painted = |session: &Session| -> Vec<Combo> {
            Combo::all()
                .filter(|combo| session.active().groups.get(*combo, &session.cache) == blue)
                .collect()
        };
        let best = painted(&session);
        assert!(!best.is_empty());
        assert!((best.len() as f64) < whole);

        // And the worst fifth is a different set of hands, all of them worth
        // less than the best fifth.
        session.clear_groups();
        assert!(session.paint_stat_part(top_pair, 80.0, 100.0, blue));
        let worst = painted(&session);
        assert!(!worst.is_empty());
        assert!(
            worst.iter().all(|combo| !best.contains(combo)),
            "the two ends overlap"
        );

        let equity = session.equity_by_combo().expect("two ranges on a board");
        let value = |combo: &Combo| equity.equity[combo.index() as usize];
        let floor = best.iter().map(value).fold(f32::INFINITY, f32::min);
        let ceiling = worst.iter().map(value).fold(f32::NEG_INFINITY, f32::max);
        assert!(ceiling <= floor, "worst {ceiling} against best {floor}");
    }

    #[test]
    fn the_whole_of_a_category_is_still_painted_as_one() {
        // Nought to a hundred is what a press has always done, and it has to
        // stay that: a category painted as a category follows the board as the
        // cards come, and a category painted hand by hand cannot.
        let mut session = session_on("Kh 7d 2c", "22+, A2s+, K9s+");
        let top_pair = crate::stats::stat_by_key("top-pair").unwrap();
        let blue = crate::groups::colour_from_key("blue").unwrap();
        assert!(session.paint_stat_part(top_pair, 0.0, 100.0, blue));
        assert_eq!(session.mark(top_pair).key(), "blue");
    }

    #[test]
    fn the_slice_can_be_taken_from_anywhere_in_the_range() {
        // One handle only ever reached the top, and the top is not the only
        // part of a range worth looking at - the hands that are neither good
        // enough to raise nor bad enough to fold are in the middle.
        let mut session = session_on("Kh 7d 2c", "22+, A2s+, K9s+, A8o+, KJo+");
        session.set_active(1);
        session
            .set_active_range_text("22+, A2s+, K2s+, Q8s+, J9s+, A2o+, K9o+")
            .unwrap();
        session.set_active(0);

        let top = session.set_continue_by_equity(0.3).expect("a slice");
        let middle = session
            .set_continue_between(0.3, 0.7)
            .expect("a slice of the middle");
        assert_eq!(middle.from, 0.3);
        assert!((middle.share - 0.4).abs() < 1e-9);
        assert!(middle.covered > 0.0);

        // The middle is worth less than the top and does not overlap it: a
        // slice is a run of the range, not a second helping of the same hands.
        assert!(
            middle.threshold < top.threshold,
            "{} against {}",
            middle.threshold,
            top.threshold
        );
        assert!(middle.range.intersection(&top.range).is_empty());

        // The bottom of the range reaches the worst hand there is.
        let bottom = session.set_continue_between(0.75, 1.0).expect("the tail");
        assert!(bottom.threshold <= middle.threshold);
        assert!(bottom.range.intersection(&top.range).is_empty());
        // And it says which hand its far handle came to rest on.
        assert!(bottom.hand.is_some());

        // The whole of it is no slice at all, and puts back what was painted.
        assert!(session.set_continue_between(0.0, 1.0).is_none());
        assert!(session.cut().is_none());
    }

    #[test]
    fn live_combos_drop_what_the_board_has_taken() {
        let mut session = session_on("Kc Qh Jh", "KK");
        assert_eq!(session.active().range.combo_count(), 6.0);
        assert_eq!(
            session.live_combos(),
            3.0,
            "the king on the board blocks three"
        );
        session.toggle_dead(Card::parse("Kh").unwrap());
        assert_eq!(session.live_combos(), 1.0, "a dead king leaves one");
        assert_eq!(session.breakdown().total_combos, session.live_combos());
    }

    #[test]
    fn one_range_taken_off_another_leaves_the_difference() {
        // Nothing in the interface reaches this any more - a chart carries its
        // actions apart, so a cold call is asked for rather than worked out -
        // but the arithmetic is still here and still has to be right.
        let mut session = Session::new();
        assert!(session.load_library("mtt-100bb-defend-btn-vs-co"));
        let whole = session.active().range.clone();
        assert!(session.subtract_library_chart("mtt-100bb-open-utg", false));
        let left = session.active().range.clone();

        assert!(left.combo_count() > 0.0);
        assert!(left.combo_count() < whole.combo_count());
        assert_eq!(left, whole.intersection(&left), "nothing new arrived");
        for combo in Combo::all() {
            let opens = library::chart_by_id("mtt-100bb-open-utg")
                .unwrap()
                .range()
                .unwrap();
            let want = (whole.get(combo) - opens.get(combo)).max(0.0);
            assert!((left.get(combo) - want).abs() < 1e-6, "{combo}");
        }
    }

    #[test]
    fn a_quick_button_comes_off_the_range_as_well_as_on_it() {
        let mut session = Session::new();
        session.set_active_range_text("22+, AQs+, AKo").unwrap();
        let before = session.active().range.combo_count();
        session.subtract_preset(Preset::Pairs);
        assert_eq!(
            session.active().range.get(Combo::parse("7h7s").unwrap()),
            0.0
        );
        assert_eq!(
            session.active().range.get(Combo::parse("AhKd").unwrap()),
            1.0
        );
        assert!(session.active().range.combo_count() < before);

        // Taking away what is not there changes nothing, rather than going
        // negative or emptying the matrix.
        let left = session.active().range.clone();
        session.subtract_preset(Preset::Pairs);
        assert_eq!(session.active().range, left);
    }

    #[test]
    fn the_library_loads_into_the_active_seat() {
        let mut session = Session::new();
        session.set_active(1);
        assert!(session.load_library("mtt-80bb-open-btn"));
        let loaded = session.active().range.combo_count();
        assert!(
            loaded > 650.0 && loaded < 800.0,
            "a button opens wide: {loaded}"
        );
        // It replaces rather than merges, and leaves the other seat alone.
        assert!(session.load_library("mtt-100bb-open-utg"));
        assert!(session.active().range.combo_count() < loaded);
        assert!(session.player(0).unwrap().range.is_empty());
        assert!(!session.load_library("mtt-999bb-open-utg"));
    }

    #[test]
    fn quick_buttons_add_to_the_range() {
        let mut session = Session::new();
        session.set_active_range_text("AA").unwrap();
        session.add_preset(Preset::Pairs);
        assert_eq!(session.active().range.combo_count(), 13.0 * 6.0);
        session.add_preset(Preset::Suited);
        assert_eq!(
            session.active().range.combo_count(),
            13.0 * 6.0 + 78.0 * 4.0
        );
    }

    #[test]
    fn a_colour_can_continue_part_of_the_time() {
        // Six combinations of kings, and the board gives them an overpair.
        let mut session = session_on("Qh 7d 2c", "KK");
        session.clear_groups();
        session.paint_stat(StatId::OVERPAIR, DEFAULT_COLOUR);
        assert_eq!(session.painted_combos(), 6.0);

        // Half the time is three of the six, and the filter says so.
        session.set_colour_share(DEFAULT_COLOUR, 0.5);
        assert_eq!(session.painted_combos(), 3.0);
        session.toggle_street_filter(0);
        assert!((session.street_count(0) - 3.0).abs() < 1e-9);

        // Nothing at all is the same as not painting them.
        session.set_colour_share(DEFAULT_COLOUR, 0.0);
        assert_eq!(session.painted_combos(), 0.0);

        // And the share is out of one, whatever it is asked for.
        session.set_colour_share(DEFAULT_COLOUR, 2.5);
        assert_eq!(session.colour_share(DEFAULT_COLOUR), 1.0);
        assert_eq!(session.painted_combos(), 6.0);
    }

    #[test]
    fn two_colours_continue_at_their_own_rates() {
        let mut session = session_on("Qh 7d 2c", "KK,JJ");
        session.clear_groups();
        let green = colour_from_key("green").unwrap();
        session.paint_stat(StatId::OVERPAIR, DEFAULT_COLOUR);
        session.paint_stat(StatId::PP_BELOW_TOP_CARD, green);
        // Six kings as an overpair, six jacks under the queen.
        assert_eq!(session.painted_combos(), 12.0);

        session.set_colour_share(green, 0.25);
        // Six blue and a quarter of six green.
        assert!((session.painted_combos() - 7.5).abs() < 1e-9);

        // The shares travel with a shared link, and so does the seat the panel
        // was comparing against: a link should show what the reader was seeing.
        session.set_compare_seat(Some(1));
        let snapshot = session.snapshot();
        let restored = Session::restore(&snapshot).unwrap();
        assert_eq!(restored.colour_share(green), 0.25);
        assert_eq!(restored.colour_share(DEFAULT_COLOUR), 1.0);
        assert_eq!(restored.compare_seat(), Some(1));
    }

    #[test]
    fn an_older_link_has_every_colour_passing_whole() {
        let mut session = session_on("Qh 7d 2c", "KK");
        let mut snapshot = session.snapshot();
        // A link that carries no shares at all - which is what one written
        // before filters had them looks like, and what one where every colour
        // passes whole looks like too.
        snapshot.shares.clear();
        let restored = Session::restore(&snapshot).unwrap();
        assert_eq!(restored.colour_share(DEFAULT_COLOUR), 1.0);
        session.clear_groups();
        session.paint_stat(StatId::OVERPAIR, DEFAULT_COLOUR);
        assert_eq!(session.painted_combos(), 6.0);
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_link_survives_being_written_down_and_read_back() {
        // The struct round trip below never leaves memory, so it cannot catch a
        // field that is left out of the text and has no way back in. This one
        // goes through the text, which is what a shared link actually is.
        let mut session = session_on("Kh 7h 2c", "22+,AKo,AQs");
        session.set_active(1);
        session.set_active_range_text("KK,QQ").unwrap();
        session.set_active(0);
        session.clear_groups();
        session.paint_stat(StatId::TOP_PAIR, DEFAULT_COLOUR);
        session.toggle_street_filter(0);
        session.set_colour_share(DEFAULT_COLOUR, 0.5);
        session.add_hand(Combo::parse("AsKs").unwrap());
        session.set_active(0);

        let text = serde_json::to_string(&session.snapshot()).unwrap();
        let read: Snapshot = serde_json::from_str(&text).expect("a link has to read back");
        let back = Session::restore(&read).unwrap();

        assert_eq!(back.board().to_string(), "Kh 7h 2c");
        assert_eq!(back.active_index(), 0);
        assert_eq!(back.players().len(), 3);
        assert_eq!(back.mark(StatId::TOP_PAIR).key(), "blue");
        assert!(back.street_applied(0));
        assert_eq!(back.colour_share(DEFAULT_COLOUR), 0.5);
        assert_eq!(back.lone_combo_for(2), Combo::parse("AsKs").ok());
        assert_eq!(back.players()[1].name, "Range B");
    }

    #[test]
    fn snapshots_round_trip() {
        let mut session = session_on("Kc Qh Jh", "22+,AQs+");
        session.set_active(1);
        session.set_active_range_text("KK,QQ").unwrap();
        session.set_active(0);
        session.paint_stat(StatId::TOP_PAIR, 1);
        session.paint_stat(StatId::FLUSH_DRAW, 2);
        session.toggle_street_filter(0);
        session.set_mode(BreakdownMode::Cumulative);
        session.toggle_dead(Card::parse("2d").unwrap());

        let snapshot = session.snapshot();
        let restored = Session::restore(&snapshot).unwrap();
        assert_eq!(restored.board().to_string(), session.board().to_string());
        assert_eq!(restored.dead().to_string(), session.dead().to_string());
        assert_eq!(restored.mode(), BreakdownMode::Cumulative);
        assert!(restored.filters_enabled());
        // Top pair was painted blue and then some of it repainted green as a
        // flushdraw, so its marker is a gear - which is the whole point of one.
        assert_eq!(
            restored.mark(StatId::TOP_PAIR).key(),
            session.mark(StatId::TOP_PAIR).key()
        );
        assert_eq!(restored.mark(StatId::FLUSH_DRAW).key(), "green");
        assert_eq!(restored.street_count(0), session.street_count(0));
        assert_eq!(restored.street_applied(0), session.street_applied(0));
        assert_eq!(restored.breakdown(), session.breakdown());
        assert_eq!(restored.snapshot(), snapshot);
    }

    #[test]
    fn rubbish_in_a_saved_grouping_is_ignored_rather_than_fatal() {
        let mut snapshot = session_on("Kh 7h 2c", "22+,AKo,AQs").snapshot();
        let saved = &mut snapshot.players[0];
        saved.painted = true;
        // A statistic this build does not have, a colour it does not have, and
        // a hand that is not one: a link from a future build, or a mangled one,
        // opens as much of itself as makes sense rather than refusing.
        saved.groups.cats = format!(
            "{}{}",
            saved.groups.cats,
            // Statistic 62 does not exist; colour 9 does not either.
            "z1\u{0039}1"
        );
        saved.groups.picks.push_str("__9");

        let restored = Session::restore(&snapshot).expect("nonsense is not fatal");
        // The parts it understood survived; the rest was dropped.
        assert_eq!(restored.mark(StatId::TOP_PAIR).key(), "blue");
        assert_ne!(restored.mark(StatId::FLUSH_DRAW).key(), "mixed");
    }

    #[test]
    fn the_range_can_be_cut_to_its_strongest_share_by_equity() {
        let mut session = session_on("Kc Qh Jh", "22+,A2s+,KJs+,AJo+");
        let cut = session
            .set_continue_by_equity(0.25)
            .expect("a flop is dealt");

        // A combo cannot be split, so the slice lands at or just past the ask.
        assert!(cut.covered >= 0.25, "covered {}", cut.covered);
        assert!(cut.covered < 0.27, "overshot to {}", cut.covered);
        assert_eq!(cut.board, "Kc Qh Jh");

        // The slider paints; the matrix only moves once a street is applied.
        assert!((session.pass_fraction() - 1.0).abs() < 1e-9);
        session.toggle_street_filter(0);
        assert!((session.pass_fraction() - cut.covered).abs() < 1e-9);
        session.toggle_street_filter(0);

        // Everything kept is at least as strong as everything dropped.
        let villain = Range::full();
        let equity = equity::equity_by_combo(
            &session.active().range,
            &villain,
            session.board(),
            CardSet::EMPTY,
        )
        .unwrap();
        for combo in session.stats().live().iter() {
            let value = equity.equity[combo.index() as usize];
            if session.active().range.get(combo) <= 0.0 || value < 0.0 {
                continue;
            }
            if cut.range.get(combo) > 0.0 {
                assert!(value >= cut.threshold, "{combo:?} kept below the threshold");
            } else {
                assert!(
                    value <= cut.threshold,
                    "{combo:?} dropped above the threshold"
                );
            }
        }
    }

    #[test]
    fn a_cut_keeps_draws_that_no_made_ladder_would_have() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        let cut = session
            .set_continue_by_equity(0.5)
            .expect("a flop is dealt");

        // AhJh has no made hand at all - ace high and the nut flushdraw - and it
        // continues, while four made pairs behind it do not. No ordering of the
        // statistics ladder produces that, which is why the cut is not one.
        let draw = Combo::parse("AhJh").unwrap();
        assert_eq!(session.describe_combo(draw), vec!["ace high", "flushdraw"]);
        assert!(cut.range.get(draw) > 0.0, "the nut flushdraw was folded");
        for pair in ["3c3d", "4c4d", "5c5d", "6c6d"] {
            let combo = Combo::parse(pair).unwrap();
            assert!(
                session.active().range.get(combo) > 0.0,
                "{pair} is in range"
            );
            assert_eq!(cut.range.get(combo), 0.0, "{pair} outlived the flushdraw");
        }

        // And it is the draw carrying it, not the ace: the same ace high without
        // one is dropped.
        let bare = Combo::parse("AcQc").unwrap();
        assert_eq!(
            cut.range.get(bare),
            0.0,
            "ace high alone should not continue"
        );
    }

    #[test]
    fn a_cut_is_frozen_where_a_filter_is_re_read_every_street() {
        let mut session = session_on("Kh 7h 2c", "22+,A2s+,KJs+,AJo+");
        session.set_continue_by_equity(0.5);
        let draw = Combo::parse("AhJh").unwrap();
        let chosen = session.cut().unwrap().range.clone();
        assert!(chosen.get(draw) > 0.0);

        session.push_board_card(Card::parse("2s").unwrap());
        session.push_board_card(Card::parse("3c").unwrap());

        // The hand is not a flushdraw any more, so a "flushdraw" filter would now
        // drop it. The cut still holds it, because what you continued with on the
        // flop is what you hold on the river - and it still says which board it
        // was taken on.
        assert!(!session.describe_combo(draw).contains(&"flushdraw"));
        assert_eq!(session.cut().unwrap().range, chosen);
        assert_eq!(session.cut().unwrap().board, "Kh 7h 2c");
        assert!(session.effective_range().get(draw) > 0.0);
    }

    #[test]
    fn a_cut_is_recomputed_when_a_snapshot_is_restored() {
        let mut session = session_on("Kc Qh Jh", "22+,A2s+,KJs+,AJo+");
        session.set_continue_by_equity(0.25);
        let snapshot = session.snapshot();
        assert_eq!(snapshot.players[0].cut_share, Some(0.25));

        let restored = Session::restore(&snapshot).unwrap();
        assert_eq!(restored.cut(), session.cut());
        assert!((restored.pass_fraction() - session.pass_fraction()).abs() < 1e-9);
    }

    #[test]
    fn a_combo_can_describe_itself() {
        let session = session_on("Kc Qh Jh", "AA");
        let labels = session.describe_combo(Combo::parse("AhKh").unwrap());
        assert!(labels.contains(&"top pair"), "{labels:?}");
        assert!(labels.contains(&"flushdraw"), "{labels:?}");
        // Combination rows would only repeat what the first two already said.
        assert!(!labels.iter().any(|l| l.contains('+')), "{labels:?}");
    }

    #[test]
    fn hovering_reports_the_overlap() {
        let session = session_on("Kc Qh Jh", "22+,A2s+,KJs+,AJo+");
        let panel = session.breakdown();
        let top_pair = panel.row(StatId::TOP_PAIR).unwrap().combos;
        let hovered = session.breakdown_within(StatId::TOP_PAIR.mask());
        assert!((hovered.total_combos - top_pair).abs() < 1e-9);
        let lit = session.highlight(StatId::TOP_PAIR.mask());
        assert!(lit.iter().any(|w| *w > 0.0));
    }
}
