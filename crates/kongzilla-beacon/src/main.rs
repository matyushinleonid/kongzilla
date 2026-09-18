//! Visit statistics for Kongzilla, as Prometheus metrics.
//!
//! The app is a static page, so it cannot count anything itself, and the gateway
//! in front of it can only count requests - which says how much traffic arrived,
//! not how many people, where from, or whether anyone used the thing. This fills
//! that gap and nothing more: a first-party endpoint the page reports to, a set
//! of counters, and a distinct-visitor count that is computed here because
//! Prometheus cannot compute it.
//!
//! Nothing personal is stored. See [`ident`] for how a visitor is recognised
//! within a day and forgotten at midnight.
//!
//! Two ports on purpose: `/api/` faces the internet, `/metrics` does not.

mod ident;
mod metrics;
mod visitors;

use metrics::Metrics;
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use visitors::Visitors;

/// The most a report may be, in bytes. Anything larger is somebody playing.
const MAX_BODY: usize = 1024;

/// How many reports one address may send a minute before being ignored.
const FLOOD_LIMIT: u32 = 120;

fn main() {
    let public_port = env_port("BEACON_PORT", 8080);
    let metrics_port = env_port("BEACON_METRICS_PORT", 9090);
    let own_host =
        std::env::var("BEACON_HOST").unwrap_or_else(|_| "kongzilla.leonid.sh".to_owned());
    let redis_url = std::env::var("REDIS_URL").ok();

    let visitors = Visitors::new(redis_url.as_deref());
    let shared = visitors.shared();
    let state = Arc::new(Mutex::new(State {
        metrics: Metrics::new(),
        visitors,
        salt: ident::Salt::new(),
        seen: HashMap::new(),
        window: Instant::now(),
    }));

    println!(
        "beacon: public :{public_port}, metrics :{metrics_port}, host {own_host}, \
         visitors counted {}",
        if shared {
            "in redis"
        } else {
            "in this process"
        }
    );

    let metrics_state = Arc::clone(&state);
    std::thread::spawn(move || serve_metrics(metrics_port, metrics_state));
    serve_public(public_port, own_host, state);
}

/// Everything the two servers share.
struct State {
    metrics: Metrics,
    visitors: Visitors,
    salt: ident::Salt,
    /// Reports per address in the current minute, for the flood check.
    seen: HashMap<String, u32>,
    window: Instant,
}

impl State {
    /// Whether this address has said enough for one minute.
    fn flooding(&mut self, address: &str) -> bool {
        if self.window.elapsed() >= Duration::from_secs(60) {
            self.seen.clear();
            self.window = Instant::now();
        }
        let count = self.seen.entry(address.to_owned()).or_default();
        *count += 1;
        *count > FLOOD_LIMIT
    }
}

/// The endpoint the page reports to.
fn serve_public(port: u16, own_host: String, state: Arc<Mutex<State>>) {
    let server = match tiny_http::Server::http(("0.0.0.0", port)) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("beacon: cannot listen on {port}: {error}");
            std::process::exit(1);
        }
    };
    for mut request in server.incoming_requests() {
        let path = request.url().split('?').next().unwrap_or("").to_owned();
        let method = request.method().as_str().to_owned();

        if path == "/healthz" {
            let _ = request.respond(text(200, "ok\n"));
            continue;
        }
        if path != "/api/event" {
            let _ = request.respond(text(404, "not found\n"));
            continue;
        }
        // A beacon is fire-and-forget: the page never waits for an answer and
        // never shows one, so every outcome is 204 and the reasons are counters.
        if method != "POST" {
            let _ = request.respond(empty(405));
            continue;
        }

        let headers: Vec<(String, String)> = request
            .headers()
            .iter()
            .map(|header| {
                (
                    header.field.as_str().as_str().to_ascii_lowercase(),
                    header.value.as_str().to_owned(),
                )
            })
            .collect();
        let header = |name: &str| {
            headers
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
                .unwrap_or("")
        };
        let address = client_address(&headers, request.remote_addr());
        let agent = header("user-agent").to_owned();
        let referrer = header("referer").to_owned();

        let mut body = String::new();
        let limited = request.as_reader().take(MAX_BODY as u64 + 1);
        let read = std::io::BufReader::new(limited).read_to_string(&mut body);
        let _ = request.respond(empty(204));

        if read.is_err() || body.len() > MAX_BODY {
            reject(&state, "malformed");
            continue;
        }
        handle(&state, &own_host, &address, &agent, &referrer, &body);
    }
}

/// Counts one report.
fn handle(
    state: &Arc<Mutex<State>>,
    own_host: &str,
    address: &str,
    agent: &str,
    referrer: &str,
    body: &str,
) {
    let Ok(report) = serde_json::from_str::<Report>(body) else {
        reject(state, "malformed");
        return;
    };
    if metrics::is_bot(agent) {
        reject(state, "bot");
        return;
    }

    let Ok(mut state) = state.lock() else { return };
    if state.flooding(address) {
        state.metrics.reject("flooding");
        return;
    }
    if !state.metrics.record(&report.name) {
        return;
    }
    if report.name == "pageview" {
        state.metrics.record_path(&report.path);
        // The header is the honest one; the body's is a courtesy for browsers
        // that strip it, and either way only the host survives.
        let source = if referrer.is_empty() {
            report.referrer.as_str()
        } else {
            referrer
        };
        let host = metrics::referrer_host(source, own_host);
        state.metrics.record_referrer(&host);

        let day = ident::today();
        let salt = *state.salt.for_day(day);
        let fingerprint = ident::fingerprint(&salt, address, agent);
        state.visitors.see(day, &fingerprint);
    }
}

fn reject(state: &Arc<Mutex<State>>, reason: &str) {
    if let Ok(mut state) = state.lock() {
        state.metrics.reject(reason);
    }
}

/// The port Prometheus scrapes. Never routed from outside the cluster.
fn serve_metrics(port: u16, state: Arc<Mutex<State>>) {
    let server = match tiny_http::Server::http(("0.0.0.0", port)) {
        Ok(server) => server,
        Err(error) => {
            eprintln!("beacon: cannot listen on {port}: {error}");
            std::process::exit(1);
        }
    };
    for request in server.incoming_requests() {
        if request.url().split('?').next() != Some("/metrics") {
            let _ = request.respond(text(404, "not found\n"));
            continue;
        }
        let body = match state.lock() {
            Ok(mut state) => {
                let today = ident::today();
                let counts = state.visitors.counts(today);
                let saturated = state.visitors.saturated();
                let up = state.visitors.store_up();
                state.metrics.set_visitors(counts, saturated);
                state.metrics.set_store_up(up);
                state.metrics.render()
            }
            Err(_) => String::new(),
        };
        let _ = request.respond(
            tiny_http::Response::from_string(body).with_header(
                tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"text/plain; version=0.0.4; charset=utf-8"[..],
                )
                .expect("a valid header"),
            ),
        );
    }
}

/// What the page sends.
#[derive(serde::Deserialize)]
struct Report {
    name: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    referrer: String,
}

/// The address to count a visitor under.
///
/// Envoy sets `x-envoy-external-address` to the address it decided is the real
/// client, having already applied its own trusted-hop rules. That is the one to
/// use: the left of `X-Forwarded-For` is whatever the client felt like claiming.
fn client_address(headers: &[(String, String)], peer: Option<&std::net::SocketAddr>) -> String {
    let find = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    };
    if let Some(address) = find("x-envoy-external-address") {
        if !address.trim().is_empty() {
            return address.trim().to_owned();
        }
    }
    if let Some(forwarded) = find("x-forwarded-for") {
        if let Some(last) = forwarded.rsplit(',').next() {
            if !last.trim().is_empty() {
                return last.trim().to_owned();
            }
        }
    }
    peer.map(|address| address.ip().to_string())
        .unwrap_or_default()
}

fn env_port(name: &str, fallback: u16) -> u16 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

fn empty(status: u16) -> tiny_http::Response<std::io::Empty> {
    tiny_http::Response::empty(status)
}

fn text(status: u16, body: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
    tiny_http::Response::from_string(body).with_status_code(status)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> Arc<Mutex<State>> {
        Arc::new(Mutex::new(State {
            metrics: Metrics::new(),
            visitors: Visitors::new(None),
            salt: ident::Salt::new(),
            seen: HashMap::new(),
            window: Instant::now(),
        }))
    }

    fn rendered(state: &Arc<Mutex<State>>) -> String {
        let mut state = state.lock().unwrap();
        let today = ident::today();
        let counts = state.visitors.counts(today);
        state.metrics.set_visitors(counts, false);
        state.metrics.render()
    }

    #[test]
    fn a_page_view_counts_a_visitor_a_path_and_a_referrer() {
        let state = state();
        handle(
            &state,
            "kongzilla.leonid.sh",
            "203.0.113.7",
            "Mozilla/5.0 Safari",
            "https://news.ycombinator.com/item?id=1",
            r#"{"name":"pageview","path":"/guide/"}"#,
        );
        let text = rendered(&state);
        assert!(text.contains("kongzilla_events_total{name=\"pageview\"} 1"));
        assert!(text.contains("kongzilla_pageviews_total{path=\"/guide/\"} 1"));
        assert!(text.contains("kongzilla_referrers_total{host=\"ycombinator.com\"} 1"));
        assert!(text.contains(&format!(
            "kongzilla_visitors{{day=\"{}\"}} 1",
            ident::day_label(ident::today())
        )));
    }

    #[test]
    fn the_same_person_reading_two_pages_is_one_visitor() {
        let state = state();
        for path in ["/", "/guide/"] {
            handle(
                &state,
                "kongzilla.leonid.sh",
                "203.0.113.7",
                "Mozilla/5.0 Safari",
                "",
                &format!(r#"{{"name":"pageview","path":"{path}"}}"#),
            );
        }
        let text = rendered(&state);
        assert!(text.contains("kongzilla_events_total{name=\"pageview\"} 2"));
        assert!(text.contains(&format!(
            "kongzilla_visitors{{day=\"{}\"}} 1",
            ident::day_label(ident::today())
        )));
    }

    #[test]
    fn robots_are_counted_as_robots_and_not_as_people() {
        let state = state();
        handle(
            &state,
            "kongzilla.leonid.sh",
            "203.0.113.9",
            "Googlebot/2.1",
            "",
            r#"{"name":"pageview","path":"/"}"#,
        );
        let text = rendered(&state);
        assert!(text.contains("kongzilla_beacon_rejected_total{reason=\"bot\"} 1"));
        assert!(text.contains("kongzilla_events_total{name=\"pageview\"} 0"));
        assert!(text.contains(&format!(
            "kongzilla_visitors{{day=\"{}\"}} 0",
            ident::day_label(ident::today())
        )));
    }

    #[test]
    fn rubbish_is_refused_without_inventing_a_series_for_it() {
        let state = state();
        // Unparseable, and parseable but naming something this build does not
        // count. Neither may leave a trace in the registry.
        for body in ["not json", "{}", r#"{"name":"'; DROP TABLE"}"#] {
            handle(
                &state,
                "kongzilla.leonid.sh",
                "203.0.113.1",
                "Safari",
                "",
                body,
            );
        }
        let text = rendered(&state);
        assert!(!text.contains("DROP TABLE"));
        assert!(text.contains("kongzilla_beacon_rejected_total{reason=\"malformed\"} 2"));
        assert!(text.contains("kongzilla_beacon_rejected_total{reason=\"unknown_event\"} 1"));
    }

    #[test]
    fn one_address_cannot_run_the_counters_up_forever() {
        let state = state();
        for _ in 0..FLOOD_LIMIT + 50 {
            handle(
                &state,
                "kongzilla.leonid.sh",
                "203.0.113.5",
                "Safari",
                "",
                r#"{"name":"flop_dealt"}"#,
            );
        }
        let text = rendered(&state);
        assert!(text.contains(&format!(
            "kongzilla_events_total{{name=\"flop_dealt\"}} {FLOOD_LIMIT}"
        )));
        assert!(text.contains("kongzilla_beacon_rejected_total{reason=\"flooding\"} 50"));
    }

    #[test]
    fn the_address_comes_from_the_gateway_not_from_the_client() {
        let headers = |pairs: &[(&str, &str)]| -> Vec<(String, String)> {
            pairs
                .iter()
                .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
                .collect()
        };
        assert_eq!(
            client_address(
                &headers(&[
                    ("x-forwarded-for", "1.2.3.4, 203.0.113.7"),
                    ("x-envoy-external-address", "203.0.113.7"),
                ]),
                None
            ),
            "203.0.113.7"
        );
        // Without Envoy's own header, the last hop is the one that was not
        // chosen by whoever sent the request.
        assert_eq!(
            client_address(
                &headers(&[("x-forwarded-for", "1.2.3.4, 203.0.113.7")]),
                None
            ),
            "203.0.113.7"
        );
        assert_eq!(client_address(&headers(&[]), None), "");
    }

    #[test]
    fn every_rejection_reason_is_one_the_registry_knows() {
        let state = state();
        for reason in metrics::REJECTIONS {
            reject(&state, reason);
        }
        let text = rendered(&state);
        for reason in metrics::REJECTIONS {
            assert!(text.contains(&format!(
                "kongzilla_beacon_rejected_total{{reason=\"{reason}\"}} 1"
            )));
        }
    }
}
