//! What is counted, and the shape Prometheus reads it in.
//!
//! Every label here is drawn from a fixed set. That is not tidiness: a label
//! taken from the request - a path, a referrer, a user agent - is a new time
//! series per distinct value, and a public endpoint will happily be fed a
//! million of them. Anything unrecognised becomes `other`, so the worst a
//! visitor can do is increment a counter that already exists.

use std::collections::BTreeMap;
use std::fmt::Write;

/// The events the page is allowed to report.
///
/// Page views say someone arrived. These say the tool was used, which is the
/// thing actually worth knowing.
pub const EVENTS: &[&str] = &[
    "pageview",
    "flop_dealt",
    "chart_loaded",
    "street_filter",
    "paint_top",
    "board_cleared",
    "copy_link",
    "copy_range",
    "session_saved",
    "image_saved",
    "preflop_run",
];

/// The paths a page view is allowed to name.
pub const PATHS: &[&str] = &[
    "/",
    "/guide/",
    "/flopzilla-alternative/",
    "/what-is-flopzilla/",
    "/ru/rukovodstvo/",
    "/ru/analog-flopzilla/",
    "/ru/razbor-diapazonov/",
];

/// How many distinct referrer hosts are tracked before the rest become `other`.
const REFERRER_LIMIT: usize = 64;

/// Why a request was not counted.
pub const REJECTIONS: &[&str] = &["bot", "malformed", "unknown_event", "flooding"];

/// Everything the exporter holds.
#[derive(Default)]
pub struct Metrics {
    events: BTreeMap<&'static str, u64>,
    pageviews: BTreeMap<&'static str, u64>,
    referrers: BTreeMap<String, u64>,
    rejected: BTreeMap<&'static str, u64>,
    /// Today's and yesterday's distinct visitors, by date.
    visitors: BTreeMap<String, u64>,
    /// Whether the visitor count has stopped being exact. See `visitors.rs`.
    saturated: bool,
    /// Whether the shared store is answering.
    store_up: bool,
}

impl Metrics {
    /// A registry with every known series at zero.
    ///
    /// Declaring them up front means a dashboard shows `0` rather than "No
    /// data" for something that has simply not happened yet, which are two very
    /// different statements.
    pub fn new() -> Self {
        let mut metrics = Self {
            store_up: true,
            ..Self::default()
        };
        for event in EVENTS {
            metrics.events.insert(event, 0);
        }
        for path in PATHS {
            metrics.pageviews.insert(path, 0);
        }
        for reason in REJECTIONS {
            metrics.rejected.insert(reason, 0);
        }
        metrics
    }

    /// Counts one event, if it is one this build knows about.
    pub fn record(&mut self, event: &str) -> bool {
        match EVENTS.iter().find(|known| **known == event) {
            Some(known) => {
                *self.events.entry(known).or_default() += 1;
                true
            }
            None => {
                self.reject("unknown_event");
                false
            }
        }
    }

    /// Counts one page view, folding an unknown path into `other`.
    pub fn record_path(&mut self, path: &str) {
        let known = PATHS.iter().find(|known| **known == path).copied();
        *self.pageviews.entry(known.unwrap_or("other")).or_default() += 1;
    }

    /// Counts one referrer, folding the long tail into `other`.
    pub fn record_referrer(&mut self, host: &str) {
        if host.is_empty() {
            return;
        }
        if !self.referrers.contains_key(host) && self.referrers.len() >= REFERRER_LIMIT {
            *self.referrers.entry("other".to_owned()).or_default() += 1;
            return;
        }
        *self.referrers.entry(host.to_owned()).or_default() += 1;
    }

    /// Counts one request that was not counted as a visit.
    pub fn reject(&mut self, reason: &str) {
        let known = REJECTIONS
            .iter()
            .find(|known| **known == reason)
            .copied()
            .unwrap_or("malformed");
        *self.rejected.entry(known).or_default() += 1;
    }

    /// Replaces the visitor counts, which are read rather than accumulated.
    pub fn set_visitors(&mut self, counts: BTreeMap<String, u64>, saturated: bool) {
        self.visitors = counts;
        self.saturated = saturated;
    }

    /// Records whether the shared store answered.
    pub fn set_store_up(&mut self, up: bool) {
        self.store_up = up;
    }

    /// The whole registry in the Prometheus text exposition format.
    pub fn render(&self) -> String {
        let mut out = String::with_capacity(2048);
        series(
            &mut out,
            "kongzilla_events_total",
            "counter",
            "Reported events, by name.",
            "name",
            self.events.iter().map(|(key, value)| (*key, *value)),
        );
        series(
            &mut out,
            "kongzilla_pageviews_total",
            "counter",
            "Page views, by path.",
            "path",
            self.pageviews.iter().map(|(key, value)| (*key, *value)),
        );
        series(
            &mut out,
            "kongzilla_referrers_total",
            "counter",
            "Page views, by the host that sent them.",
            "host",
            self.referrers
                .iter()
                .map(|(key, value)| (key.as_str(), *value)),
        );
        series(
            &mut out,
            "kongzilla_beacon_rejected_total",
            "counter",
            "Requests that were not counted, by why not.",
            "reason",
            self.rejected.iter().map(|(key, value)| (*key, *value)),
        );
        series(
            &mut out,
            "kongzilla_visitors",
            "gauge",
            "Distinct visitors, by UTC day.",
            "day",
            self.visitors
                .iter()
                .map(|(key, value)| (key.as_str(), *value)),
        );
        let _ = writeln!(
            out,
            "# HELP kongzilla_visitors_saturated Whether the day's visitor count has hit its cap and stopped being exact.\n\
             # TYPE kongzilla_visitors_saturated gauge\n\
             kongzilla_visitors_saturated {}",
            u8::from(self.saturated)
        );
        let _ = writeln!(
            out,
            "# HELP kongzilla_beacon_store_up Whether the shared visitor store is answering.\n\
             # TYPE kongzilla_beacon_store_up gauge\n\
             kongzilla_beacon_store_up {}",
            u8::from(self.store_up)
        );
        out
    }
}

/// One metric family, with its help text and every label value it holds.
fn series<'a>(
    out: &mut String,
    name: &str,
    kind: &str,
    help: &str,
    label: &str,
    values: impl Iterator<Item = (&'a str, u64)>,
) {
    let _ = writeln!(out, "# HELP {name} {help}");
    let _ = writeln!(out, "# TYPE {name} {kind}");
    for (key, value) in values {
        let _ = writeln!(out, "{name}{{{label}=\"{}\"}} {value}", escape(key));
    }
}

/// Escapes a label value, which may contain a quote or a backslash.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(character),
        }
    }
    out
}

/// The registrable-looking part of a referrer, or empty for one to ignore.
///
/// Full URLs are not worth keeping: the query string is where the personal data
/// lives, and a path makes every search result its own time series. The host is
/// the whole of what is useful - where people are coming from.
pub fn referrer_host(referrer: &str, own_host: &str) -> String {
    let after_scheme = referrer
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(referrer);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit('@')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase();
    if host.is_empty() || host == own_host || host.ends_with(&format!(".{own_host}")) {
        // Coming from our own pages is navigation, not a referral.
        return String::new();
    }
    if !host
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
    {
        return String::new();
    }
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() <= 2 {
        return host;
    }
    // Enough of the tail to name the site, without one series per subdomain.
    labels[labels.len() - 2..].join(".")
}

/// Whether a user agent is something that should not be counted as a visit.
pub fn is_bot(agent: &str) -> bool {
    const MARKERS: &[&str] = &[
        "bot",
        "crawl",
        "spider",
        "slurp",
        "curl",
        "wget",
        "python-requests",
        "headlesschrome",
        "phantomjs",
        "monitor",
        "uptime",
        "probe",
        "scanner",
        "lighthouse",
        "pingdom",
        "go-http-client",
        "java/",
        "okhttp",
        "postman",
    ];
    let agent = agent.to_ascii_lowercase();
    agent.is_empty() || MARKERS.iter().any(|marker| agent.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_known_series_is_declared_before_anything_happens() {
        let metrics = Metrics::new();
        let text = metrics.render();
        for event in EVENTS {
            assert!(
                text.contains(&format!("kongzilla_events_total{{name=\"{event}\"}} 0")),
                "{event} is missing"
            );
        }
        assert!(text.contains("kongzilla_pageviews_total{path=\"/guide/\"} 0"));
        assert!(text.contains("kongzilla_beacon_store_up 1"));
    }

    #[test]
    fn an_unknown_event_is_refused_rather_than_given_a_series() {
        let mut metrics = Metrics::new();
        assert!(metrics.record("flop_dealt"));
        assert!(!metrics.record("../../etc/passwd"));
        let text = metrics.render();
        assert!(text.contains("kongzilla_events_total{name=\"flop_dealt\"} 1"));
        assert!(!text.contains("passwd"));
        assert!(text.contains("kongzilla_beacon_rejected_total{reason=\"unknown_event\"} 1"));
    }

    #[test]
    fn an_unknown_path_lands_in_other_rather_than_its_own_series() {
        let mut metrics = Metrics::new();
        metrics.record_path("/guide/");
        for index in 0..500 {
            metrics.record_path(&format!("/made/up/{index}"));
        }
        let text = metrics.render();
        assert!(text.contains("kongzilla_pageviews_total{path=\"/guide/\"} 1"));
        assert!(text.contains("kongzilla_pageviews_total{path=\"other\"} 500"));
        assert_eq!(
            text.matches("kongzilla_pageviews_total{").count(),
            PATHS.len() + 1
        );
    }

    #[test]
    fn the_referrer_tail_is_bounded() {
        let mut metrics = Metrics::new();
        for index in 0..500 {
            metrics.record_referrer(&format!("host{index}.example"));
        }
        let text = metrics.render();
        assert_eq!(
            text.matches("kongzilla_referrers_total{").count(),
            REFERRER_LIMIT + 1
        );
        assert!(text.contains("kongzilla_referrers_total{host=\"other\"} 436"));
    }

    #[test]
    fn a_referrer_becomes_a_site_not_a_url() {
        let own = "kongzilla.leonid.sh";
        assert_eq!(
            referrer_host("https://www.google.com/search?q=a+b", own),
            "google.com"
        );
        assert_eq!(
            referrer_host("https://news.ycombinator.com/item?id=1", own),
            "ycombinator.com"
        );
        assert_eq!(
            referrer_host("https://reddit.com/r/poker", own),
            "reddit.com"
        );
        // Our own pages are navigation, not a referral.
        assert_eq!(referrer_host("https://kongzilla.leonid.sh/guide/", own), "");
        assert_eq!(referrer_host("", own), "");
        // Nothing that could smuggle a label value through.
        assert_eq!(referrer_host("https://evil\"\ncom/", own), "");
    }

    #[test]
    fn label_values_cannot_break_the_exposition_format() {
        let mut metrics = Metrics::new();
        metrics.record_referrer("a\"b\\c");
        let text = metrics.render();
        assert!(text.contains(r#"kongzilla_referrers_total{host="a\"b\\c"} 1"#));
    }

    #[test]
    fn the_usual_robots_are_recognised() {
        for agent in [
            "Googlebot/2.1",
            "Mozilla/5.0 (compatible; bingbot/2.0)",
            "curl/8.5.0",
            "python-requests/2.31",
            "",
        ] {
            assert!(is_bot(agent), "{agent} should not count as a visit");
        }
        assert!(!is_bot(
            "Mozilla/5.0 (Macintosh) AppleWebKit/605.1.15 Safari/605.1.15"
        ));
    }
}
