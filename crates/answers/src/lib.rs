//! Instant answers: maths, units, currency, time zones. Owner: answers agent.
use ss_core::Answer;

/// Exchange rates against the euro, with the date they were published.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Rates {
    pub date: String,
    pub eur: std::collections::HashMap<String, f64>,
}

/// The answer for a query, or None when it is not a question this crate answers.
/// `now_unix` and `home_tz` (an IANA name such as "Australia/Brisbane") make
/// time answers testable.
pub fn answer(q: &str, rates: Option<&Rates>, now_unix: i64, home_tz: &str) -> Option<Answer> {
    let _ = (q, rates, now_unix, home_tz);
    None
}

/// Rates cached under `cache`, refreshed at most twice a day when online.
pub fn rates(cache: &std::path::Path) -> Option<Rates> {
    let _ = cache;
    None
}
