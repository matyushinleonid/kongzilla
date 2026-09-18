//! The text format used to move ranges between tools.
//!
//! This is the notation Equilab, PokerStove and Flopzilla all speak, plus an
//! optional `:weight` suffix for mixed strategies:
//!
//! ```text
//! AKs+, 77-99, QJo, AhKh, KQs:0.5
//! ```

use crate::cards::{Combo, HandClass, NUM_CLASSES, RANK_ACE, RANK_CHARS};
use crate::error::ParseError;
use crate::range::Range;

/// Parses a range string into per-combo weights.
pub fn parse(text: &str) -> Result<Range, ParseError> {
    if let Some(runs) = text.strip_prefix(RUNS) {
        return parse_runs(runs);
    }
    let mut range = Range::empty();
    for raw in text.split([',', ';', '\n']) {
        let token = raw.trim();
        if token.is_empty() {
            continue;
        }
        let (body, weight) = split_weight(token)?;
        apply_token(&mut range, body, weight)?;
    }
    Ok(range)
}

fn split_weight(token: &str) -> Result<(&str, f32), ParseError> {
    // A packed weight first: a link writes them, and nothing a reader types
    // contains either marker, so there is nothing to tell apart.
    for marker in [ROUND, EXACT] {
        if let Some((body, packed)) = token.split_once(marker) {
            return match unpack_weight(marker, packed) {
                Some(weight) => Ok((body.trim(), weight)),
                None => Err(ParseError::BadWeight(packed.to_owned())),
            };
        }
    }
    match token.split_once(':') {
        None => Ok((token.trim(), 1.0)),
        Some((body, weight)) => {
            let weight = weight.trim();
            let parsed: f32 = weight
                .strip_suffix('%')
                .map(|w| w.parse::<f32>().map(|v| v / 100.0))
                .unwrap_or_else(|| weight.parse::<f32>())
                .map_err(|_| ParseError::BadWeight(weight.to_owned()))?;
            if !(0.0..=1.0).contains(&parsed) {
                return Err(ParseError::BadWeight(weight.to_owned()));
            }
            Ok((body.trim(), parsed))
        }
    }
}

fn apply_token(range: &mut Range, token: &str, weight: f32) -> Result<(), ParseError> {
    let bad = || ParseError::BadRangeToken(token.to_owned());

    // An explicit combo such as `AhKh`.
    if token.len() == 4 {
        if let Ok(combo) = Combo::parse(token) {
            range.set(combo, weight);
            return Ok(());
        }
    }

    // A dash range such as `77-99` or `A5s-A2s`.
    if let Some((low, high)) = token.split_once('-') {
        let low = base_classes(low.trim())?;
        let high = base_classes(high.trim())?;
        if low.len() != high.len() {
            return Err(bad());
        }
        for (a, b) in low.into_iter().zip(high) {
            for class in span(a, b).ok_or_else(bad)? {
                range.set_class(class, weight);
            }
        }
        return Ok(());
    }

    // A plus range such as `TT+` or `A2s+`.
    if let Some(base) = token.strip_suffix('+') {
        for class in base_classes(base.trim())? {
            for class in plus_span(class) {
                range.set_class(class, weight);
            }
        }
        return Ok(());
    }

    for class in base_classes(token)? {
        range.set_class(class, weight);
    }
    Ok(())
}

fn rank_char(byte: u8) -> Option<u8> {
    RANK_CHARS
        .iter()
        .position(|&c| c == byte.to_ascii_uppercase())
        .map(|rank| rank as u8)
}

/// The classes a token names before any `+` or `-` is applied.
///
/// Two characters that are not a pair name both shapes, so `AK` is `AKs` plus
/// `AKo`, and `A2+` widens both of them.
fn base_classes(token: &str) -> Result<Vec<HandClass>, ParseError> {
    let bytes = token.as_bytes();
    let bad = || ParseError::BadRangeToken(token.to_owned());
    if bytes.len() == 2 {
        let high = rank_char(bytes[0]).ok_or_else(bad)?;
        let low = rank_char(bytes[1]).ok_or_else(bad)?;
        if high == low {
            return Ok(vec![HandClass::from_ranks(high, low, false)]);
        }
        return Ok(vec![
            HandClass::from_ranks(high, low, true),
            HandClass::from_ranks(high, low, false),
        ]);
    }
    Ok(vec![HandClass::parse(token).map_err(|_| bad())?])
}

/// Every class between `a` and `b` inclusive, walking the axis they share.
fn span(a: HandClass, b: HandClass) -> Option<Vec<HandClass>> {
    if a.is_pair() && b.is_pair() {
        let (low, high) = min_max(a.high_rank(), b.high_rank());
        return Some(
            (low..=high)
                .map(|r| HandClass::from_ranks(r, r, false))
                .collect(),
        );
    }
    if a.is_pair() != b.is_pair()
        || a.is_suited() != b.is_suited()
        || a.high_rank() != b.high_rank()
    {
        return None;
    }
    let (low, high) = min_max(a.low_rank(), b.low_rank());
    Some(
        (low..=high)
            .map(|r| HandClass::from_ranks(a.high_rank(), r, a.is_suited()))
            .collect(),
    )
}

/// Every class at or above `class`: higher pairs, or the same high card with a
/// higher kicker.
fn plus_span(class: HandClass) -> Vec<HandClass> {
    if class.is_pair() {
        let rank = class.high_rank();
        return (rank..=RANK_ACE)
            .map(|r| HandClass::from_ranks(r, r, false))
            .collect();
    }
    let (high, low, suited) = (class.high_rank(), class.low_rank(), class.is_suited());
    (low..high)
        .map(|r| HandClass::from_ranks(high, r, suited))
        .collect()
}

fn min_max(a: u8, b: u8) -> (u8, u8) {
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// Renders a range back to text, compressing runs into `+` and `-` forms.
pub fn format(range: &Range) -> String {
    write(range, with_weight)
}

/// The same range, written for a link rather than for a reader.
///
/// Two ways of saying it, and the shorter one wins. Naming the hands is best
/// when there are few to name; listing the matrix cell by cell is best when the
/// range is a solver's, where most cells carry a frequency of their own and
/// naming each one costs more than the frequency does.
///
/// Both forms parse, so the text somebody types and the text a link carries are
/// the same language.
pub fn format_packed(range: &Range) -> String {
    let named = write(range, with_packed_weight);
    match format_runs(range) {
        Some(runs) if runs.len() < named.len() => runs,
        _ => named,
    }
}

/// The marker that says a range is written as runs of matrix cells.
const RUNS: char = 'R';
/// The weight code that says the real weight follows in two characters.
const ESCAPE: usize = 63;

/// The range as runs of matrix cells, or `None` when it cannot be said that way.
///
/// A range laid out in matrix order is mostly long stretches of the same thing:
/// a row of suited aces all played, a block of small offsuit hands all folded,
/// and a thin edge of frequencies between them. Ninety-five solver charts
/// average thirty-one such stretches over a hundred and sixty-nine cells.
///
/// It cannot say a cell whose combinations disagree - a hand picked apart by
/// suit - so that range goes out under its own names instead.
fn format_runs(range: &Range) -> Option<String> {
    let mut weights = Vec::with_capacity(NUM_CLASSES);
    for index in 0..NUM_CLASSES {
        let class = HandClass::from_index(index as u8);
        let mut combos = class.combos();
        let first = range.get(combos.next()?);
        if combos.any(|combo| (range.get(combo) - first).abs() > 1e-6) {
            return None;
        }
        weights.push((first * 1000.0).round().clamp(0.0, 4095.0) as usize);
    }

    let mut out = String::from(RUNS);
    let mut at = 0;
    while at < weights.len() {
        let weight = weights[at];
        let mut run = 1;
        // A run is capped at what one character can count; a longer stretch
        // simply becomes two runs of the same weight.
        while at + run < weights.len() && weights[at + run] == weight && run < 63 {
            run += 1;
        }
        out.push(char::from(DIGITS[run]));
        if weight % 50 == 0 {
            out.push(char::from(DIGITS[weight / 50]));
        } else {
            out.push(char::from(DIGITS[ESCAPE]));
            out.push(char::from(DIGITS[weight / 64]));
            out.push(char::from(DIGITS[weight % 64]));
        }
        at += run;
    }
    Some(out)
}

/// Reads a run-encoded range back.
fn parse_runs(text: &str) -> Result<Range, ParseError> {
    let digit = |c: char| DIGITS.iter().position(|d| char::from(*d) == c);
    let bad = || ParseError::BadWeight(text.to_owned());
    let mut range = Range::empty();
    let mut chars = text.chars();
    let mut at = 0usize;

    while let Some(length) = chars.next() {
        let run = digit(length).ok_or_else(bad)?;
        let code = digit(chars.next().ok_or_else(bad)?).ok_or_else(bad)?;
        let per_mille = if code == ESCAPE {
            let high = digit(chars.next().ok_or_else(bad)?).ok_or_else(bad)?;
            let low = digit(chars.next().ok_or_else(bad)?).ok_or_else(bad)?;
            high * 64 + low
        } else {
            code * 50
        };
        let weight = (per_mille as f32 / 1000.0).clamp(0.0, 1.0);
        for index in at..(at + run).min(NUM_CLASSES) {
            if weight > 0.0 {
                for combo in HandClass::from_index(index as u8).combos() {
                    range.set(combo, weight);
                }
            }
        }
        at += run;
        if at >= NUM_CLASSES {
            break;
        }
    }
    Ok(range)
}

fn write(range: &Range, with_weight: fn(String, f32) -> String) -> String {
    let mut tokens: Vec<String> = Vec::new();
    let mut handled = [false; crate::cards::NUM_CLASSES];

    // Uniform classes first, compressed into runs.
    let uniform = |class: HandClass| -> Option<f32> {
        let mut weight = None;
        for combo in class.combos() {
            let w = range.get(combo);
            match weight {
                None => weight = Some(w),
                Some(prev) if (prev - w).abs() < 1e-6 => {}
                Some(_) => return None,
            }
        }
        weight.filter(|w| *w > 0.0)
    };

    // Pocket pairs.
    let mut rank = RANK_ACE as i8;
    while rank >= 0 {
        let class = HandClass::from_ranks(rank as u8, rank as u8, false);
        match uniform(class) {
            Some(weight) => {
                let top = rank;
                let mut bottom = rank;
                while bottom > 0 {
                    let next = HandClass::from_ranks(bottom as u8 - 1, bottom as u8 - 1, false);
                    if uniform(next).map(|w| (w - weight).abs() < 1e-6) == Some(true) {
                        bottom -= 1;
                    } else {
                        break;
                    }
                }
                for r in bottom..=top {
                    handled[HandClass::from_ranks(r as u8, r as u8, false).index() as usize] = true;
                }
                let bottom_class = HandClass::from_ranks(bottom as u8, bottom as u8, false);
                let top_class = HandClass::from_ranks(top as u8, top as u8, false);
                tokens.push(with_weight(
                    if top as u8 == RANK_ACE && bottom < top {
                        format!("{bottom_class}+")
                    } else if bottom == top {
                        bottom_class.to_string()
                    } else {
                        format!("{bottom_class}-{top_class}")
                    },
                    weight,
                ));
                rank = bottom - 1;
            }
            None => rank -= 1,
        }
    }

    // Suited and offsuit runs, grouped by high card.
    for suited in [true, false] {
        for hi in (1..=RANK_ACE).rev() {
            let mut lo = hi as i8 - 1;
            while lo >= 0 {
                let class = HandClass::from_ranks(hi, lo as u8, suited);
                match uniform(class) {
                    Some(weight) => {
                        let top = lo;
                        let mut bottom = lo;
                        while bottom > 0 {
                            let next = HandClass::from_ranks(hi, bottom as u8 - 1, suited);
                            if uniform(next).map(|w| (w - weight).abs() < 1e-6) == Some(true) {
                                bottom -= 1;
                            } else {
                                break;
                            }
                        }
                        for r in bottom..=top {
                            handled[HandClass::from_ranks(hi, r as u8, suited).index() as usize] =
                                true;
                        }
                        let bottom_class = HandClass::from_ranks(hi, bottom as u8, suited);
                        let top_class = HandClass::from_ranks(hi, top as u8, suited);
                        tokens.push(with_weight(
                            if top as u8 == hi - 1 && bottom < top {
                                format!("{bottom_class}+")
                            } else if bottom == top {
                                bottom_class.to_string()
                            } else {
                                format!("{bottom_class}-{top_class}")
                            },
                            weight,
                        ));
                        lo = bottom - 1;
                    }
                    None => lo -= 1,
                }
            }
        }
    }

    // Anything left over is written combo by combo.
    for class in HandClass::all() {
        if handled[class.index() as usize] {
            continue;
        }
        for combo in class.combos() {
            let weight = range.get(combo);
            if weight > 0.0 {
                tokens.push(with_weight(combo.to_string(), weight));
            }
        }
    }

    tokens.join(",")
}

fn with_weight(token: String, weight: f32) -> String {
    if (weight - 1.0).abs() < 1e-6 {
        token
    } else {
        format!("{token}:{}", trim_float(weight))
    }
}

/// The alphabet a packed weight is written in: URL-safe, one character each.
const DIGITS: &[u8; 64] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz-_";

/// A weight that is a whole twentieth: one character.
const ROUND: char = '~';
/// Any other weight: two characters of thousandths.
const EXACT: char = '=';

/// A weight in one or two characters rather than six.
///
/// `:0.379` is six characters of a shared link for one number, and not every
/// number needs six. A weight the reader made is a twentieth - the slider moves
/// in twentieths and the brush in quarters - and a twentieth is one character
/// of a sixty-four letter alphabet. Only a solver's own frequency needs more,
/// and two characters hold it to the thousandth, which is the precision the
/// charts are stored at.
///
/// So the common case costs two characters and the rare one three, which is the
/// right way round: a third of the characters in a chart go on these.
fn pack_weight(weight: f32) -> String {
    let per_mille = (weight * 1000.0).round().clamp(0.0, 4095.0) as usize;
    let mut out = String::with_capacity(3);
    if per_mille % 50 == 0 {
        out.push(ROUND);
        out.push(char::from(DIGITS[per_mille / 50]));
    } else {
        out.push(EXACT);
        out.push(char::from(DIGITS[per_mille / 64]));
        out.push(char::from(DIGITS[per_mille % 64]));
    }
    out
}

/// Reads one back, or `None` for anything that is not one.
fn unpack_weight(marker: char, text: &str) -> Option<f32> {
    let digit = |c: char| DIGITS.iter().position(|d| char::from(*d) == c);
    let mut chars = text.chars();
    let per_mille = match marker {
        ROUND => {
            let only = digit(chars.next()?)?;
            only.checked_mul(50)?
        }
        _ => {
            let (high, low) = (digit(chars.next()?)?, digit(chars.next()?)?);
            high * 64 + low
        }
    };
    if chars.next().is_some() {
        return None;
    }
    Some((per_mille as f32 / 1000.0).clamp(0.0, 1.0))
}

fn with_packed_weight(token: String, weight: f32) -> String {
    if (weight - 1.0).abs() < 1e-6 {
        token
    } else {
        format!("{token}{}", pack_weight(weight))
    }
}

fn trim_float(value: f32) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    text.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(text: &str) -> f64 {
        parse(text).unwrap().combo_count()
    }

    #[test]
    fn single_classes_parse() {
        assert_eq!(count("AA"), 6.0);
        assert_eq!(count("AKs"), 4.0);
        assert_eq!(count("AKo"), 12.0);
        assert_eq!(count("AK"), 16.0);
        assert_eq!(count("AhKh"), 1.0);
    }

    #[test]
    fn plus_and_dash_ranges_parse() {
        assert_eq!(count("TT+"), 5.0 * 6.0);
        assert_eq!(count("77-99"), 3.0 * 6.0);
        assert_eq!(count("99-77"), 3.0 * 6.0);
        assert_eq!(count("A2s+"), 12.0 * 4.0);
        assert_eq!(count("KTs+"), 3.0 * 4.0);
        assert_eq!(count("A2s-A5s"), 4.0 * 4.0);
    }

    #[test]
    fn weights_parse() {
        let range = parse("AA:0.5").unwrap();
        assert!((range.combo_count() - 3.0).abs() < 1e-6);
        let range = parse("AA:50%").unwrap();
        assert!((range.combo_count() - 3.0).abs() < 1e-6);
        assert!(parse("AA:2").is_err());
        assert!(parse("AA:x").is_err());
    }

    #[test]
    fn whole_deck_is_1326_combos() {
        assert_eq!(
            count("22+, A2+, K2+, Q2+, J2+, T2+, 92+, 82+, 72+, 62+, 52+, 42+, 32+"),
            1326.0
        );
    }

    #[test]
    fn every_library_chart_survives_being_packed() {
        // The charts are the ranges links are made of, and the run form is only
        // worth having if every one of them comes back exactly.
        let mut named = 0usize;
        let mut runs = 0usize;
        let mut total = 0usize;
        for chart in crate::library::charts() {
            let Ok(range) = chart.range() else { continue };
            let packed = format_packed(&range);
            let back = parse(&packed).unwrap_or_else(|e| panic!("{}: {e}", chart.id));
            for combo in crate::cards::Combo::all() {
                assert!(
                    (range.get(combo) - back.get(combo)).abs() < 1e-6,
                    "{} lost {combo}",
                    chart.id
                );
            }
            total += packed.len();
            if packed.starts_with(RUNS) {
                runs += 1;
            } else {
                named += 1;
            }
        }
        let charts = named + runs;
        assert!(charts > 90, "the library should have charts in it");
        // The run form should be winning on most of them; if it stopped, the
        // shape this was built for has changed and the trade is worth re-reading.
        assert!(runs > named, "{runs} runs against {named} named");
        assert!(
            total / charts < 120,
            "mean packed length {}",
            total / charts
        );
    }

    #[test]
    fn a_packed_weight_is_the_same_weight() {
        // Every weight the app can make is a whole number of thousandths, so
        // packing one and reading it back has to give exactly it.
        for per_mille in [1u32, 25, 50, 250, 379, 400, 420, 500, 750, 950, 999] {
            let weight = per_mille as f32 / 1000.0;
            let text = format_packed(&parse(&format!("AhKh:{weight}")).unwrap());
            let back = parse(&text).unwrap();
            assert!(
                (back.get(Combo::parse("AhKh").unwrap()) - weight).abs() < 1e-6,
                "{per_mille} came back as {text}"
            );
        }
    }

    #[test]
    fn a_packed_range_says_the_same_thing_in_fewer_characters() {
        let range = parse("22+,A2s+,Q7o:0.379,T7o:0.42,97o:0.399").unwrap();
        let human = format(&range);
        let packed = format_packed(&range);
        assert_eq!(parse(&packed).unwrap(), range, "packed reads back the same");
        assert!(
            packed.len() < human.len(),
            "packed {packed} is no shorter than {human}"
        );
        // `:0.379` and `:0.399` are not twentieths and go from six characters
        // to three; `:0.42` is not one either and goes from five to three.
        assert_eq!(human.len() - packed.len(), 8);
    }

    #[test]
    fn formatting_round_trips() {
        for text in [
            "AA",
            "TT+,AKs,AQs,AKo",
            "77-99,A2s-A5s,QJo",
            "AhKh,AsKs",
            "AA:0.5,KK",
        ] {
            let parsed = parse(text).unwrap();
            let printed = format(&parsed);
            let reparsed = parse(&printed).unwrap();
            assert_eq!(parsed, reparsed, "{text} -> {printed}");
        }
    }

    #[test]
    fn formatting_compresses_runs() {
        assert_eq!(format(&parse("TT,JJ,QQ,KK,AA").unwrap()), "TT+");
        assert_eq!(format(&parse("77,88,99").unwrap()), "77-99");
        assert_eq!(format(&parse("AKs,AQs,AJs").unwrap()), "AJs+");
        assert_eq!(format(&Range::empty()), "");
    }

    #[test]
    fn bad_tokens_are_rejected() {
        assert!(parse("XX").is_err());
        assert!(parse("AKz").is_err());
        assert!(parse("77-AKs").is_err());
    }
}
