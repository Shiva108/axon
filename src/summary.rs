//! The `--scan-only` JSON summary (DESIGN.md §14 M1): headline totals plus per-model and
//! per-agent breakdowns, the unpriced-model list (surfaced loudly, never a silent €0), and
//! the attribution honesty gauge (§15.3) — the share of output tokens we could **not**
//! attribute to a named agent.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::model::{Event, PricingKind};
use crate::rtk::RtkSavings;

#[derive(Debug, Clone, Serialize)]
pub struct ModelStat {
    pub model: String,
    pub events: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub money_tokens_out: u64,
    pub credit_tokens_out: u64,
    pub cost_eur: f64,
    pub cost_credits: Option<f64>,
    pub pricing_kind: String,
    pub unpriced: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentStat {
    pub agent: String,
    pub is_subagent: bool,
    pub events: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_eur: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct HarnessStat {
    pub harness: String,
    pub events: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cost_eur: f64,
}

/// One row of the live feed — a real recent turn, trimmed to what the dashboard shows
/// (no project/path, so nothing identifying leaks into the feed).
#[derive(Debug, Clone, Serialize)]
pub struct RecentEvent {
    pub ts: i64,
    pub harness: String,
    pub agent: String,
    pub model: String,
    pub tokens_out: u64,
    pub cost_eur: f64,
    pub cost_credits: Option<f64>,
    pub pricing_kind: String,
    pub duration_ms: Option<u64>,
}

/// The `n` most recent events (newest first) as feed rows. Cheap: collect + sort by ts.
pub fn recent_events<'a>(
    events: impl IntoIterator<Item = &'a Event>,
    n: usize,
) -> Vec<RecentEvent> {
    let mut v: Vec<&Event> = events.into_iter().collect();
    v.sort_by_key(|e| std::cmp::Reverse(e.ts)); // newest first
    v.into_iter()
        .take(n)
        .map(|e| RecentEvent {
            ts: e.ts,
            harness: e.harness.as_str().to_string(),
            agent: e.agent.clone(),
            model: e.model.clone(),
            tokens_out: e.tokens_out,
            cost_eur: e.cost_eur,
            cost_credits: e.cost_credits,
            pricing_kind: e.pricing_kind.as_str().to_string(),
            duration_ms: e.duration_ms,
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
pub struct Summary {
    pub events: u64,
    pub sessions: u64,
    pub tokens_in: u64,
    pub tokens_out: u64,
    pub cache_read: u64,
    pub cache_write_5m: u64,
    pub cache_write_1h: u64,
    pub loc_added: u64,
    pub loc_removed: u64,
    pub cost_eur: f64,
    pub total_credits: f64,
    /// Canonical ids of models missing from the pricing map (their cost is a floor).
    pub unpriced_models: Vec<String>,
    pub credit_priced_models: Vec<String>,
    pub preview_priced_models: Vec<String>,
    /// Honesty gauge (§15.3): % of output tokens attributed to `unknown-sub`/`unknown`.
    pub unattributed_token_pct: f64,
    pub by_model: Vec<ModelStat>,
    pub by_agent: Vec<AgentStat>,
    pub by_harness: Vec<HarnessStat>,
    /// Optional RTK (token-saver) analytics; `None` if rtk is not installed.
    pub rtk: Option<RtkSavings>,
    /// Spend in the current local day / week / month (display currency).
    pub today_cost_eur: f64,
    pub week_cost_eur: f64,
    pub month_cost_eur: f64,
    /// Optional soft budget caps from `config.toml`.
    pub budget_day_eur: Option<f64>,
    pub budget_week_eur: Option<f64>,
    pub budget_month_eur: Option<f64>,
    /// Newest turns for the live feed (filled by the server per request; empty in `--scan-only`).
    pub recent: Vec<RecentEvent>,
}

/// Aggregate any set of events (a slice, or a filtered iterator for a time range).
pub fn build_summary<'a>(events: impl IntoIterator<Item = &'a Event>) -> Summary {
    let mut s = Summary {
        events: 0,
        sessions: 0,
        tokens_in: 0,
        tokens_out: 0,
        cache_read: 0,
        cache_write_5m: 0,
        cache_write_1h: 0,
        loc_added: 0,
        loc_removed: 0,
        cost_eur: 0.0,
        total_credits: 0.0,
        unpriced_models: Vec::new(),
        credit_priced_models: Vec::new(),
        preview_priced_models: Vec::new(),
        unattributed_token_pct: 0.0,
        by_model: Vec::new(),
        by_agent: Vec::new(),
        by_harness: Vec::new(),
        rtk: None,
        today_cost_eur: 0.0,
        week_cost_eur: 0.0,
        month_cost_eur: 0.0,
        budget_day_eur: None,
        budget_week_eur: None,
        budget_month_eur: None,
        recent: Vec::new(),
    };

    let mut sessions = std::collections::BTreeSet::new();
    let mut models: BTreeMap<String, ModelStat> = BTreeMap::new();
    let mut agents: BTreeMap<String, AgentStat> = BTreeMap::new();
    let mut harnesses: BTreeMap<String, HarnessStat> = BTreeMap::new();
    let mut credit_models = std::collections::BTreeSet::new();
    let mut preview_models = std::collections::BTreeSet::new();
    let mut unattributed_out: u64 = 0;

    for e in events {
        s.events += 1;
        s.tokens_in += e.tokens_in;
        s.tokens_out += e.tokens_out;
        s.cache_read += e.cache_read;
        s.cache_write_5m += e.cache_write_5m;
        s.cache_write_1h += e.cache_write_1h;
        s.loc_added += e.loc_added as u64;
        s.loc_removed += e.loc_removed as u64;
        s.cost_eur += e.cost_eur;
        s.total_credits += e.cost_credits.unwrap_or(0.0);
        sessions.insert(e.session_id.clone());

        if matches!(e.agent.as_str(), "unknown-sub" | "unknown") {
            unattributed_out += e.tokens_out;
        }

        let m = models.entry(e.model.clone()).or_insert_with(|| ModelStat {
            model: e.model.clone(),
            events: 0,
            tokens_in: 0,
            tokens_out: 0,
            money_tokens_out: 0,
            credit_tokens_out: 0,
            cost_eur: 0.0,
            cost_credits: None,
            pricing_kind: e.pricing_kind.as_str().to_string(),
            unpriced: e.unpriced,
        });
        m.events += 1;
        m.tokens_in += e.tokens_in;
        m.tokens_out += e.tokens_out;
        m.cost_eur += e.cost_eur;
        m.cost_credits = Some(m.cost_credits.unwrap_or(0.0) + e.cost_credits.unwrap_or(0.0))
            .filter(|v| *v > 0.0);
        match e.pricing_kind {
            PricingKind::ApiMoney | PricingKind::ReportedCost if e.cost_eur > 0.0 => {
                m.money_tokens_out += e.tokens_out;
            }
            PricingKind::ChatgptIncluded if e.cost_credits.unwrap_or(0.0) > 0.0 => {
                m.credit_tokens_out += e.tokens_out;
            }
            _ => {}
        }
        if m.pricing_kind != e.pricing_kind.as_str() {
            m.pricing_kind = "mixed".to_string();
        }
        m.unpriced |= e.unpriced;
        if e.pricing_kind == PricingKind::ChatgptIncluded {
            credit_models.insert(e.model.clone());
        }
        if e.pricing_kind == PricingKind::ChatgptPreview {
            preview_models.insert(e.model.clone());
        }

        let a = agents.entry(e.agent.clone()).or_insert_with(|| AgentStat {
            agent: e.agent.clone(),
            is_subagent: e.is_subagent,
            events: 0,
            tokens_in: 0,
            tokens_out: 0,
            cost_eur: 0.0,
        });
        a.events += 1;
        a.tokens_in += e.tokens_in;
        a.tokens_out += e.tokens_out;
        a.cost_eur += e.cost_eur;

        let h = harnesses
            .entry(e.harness.as_str().to_string())
            .or_insert_with(|| HarnessStat {
                harness: e.harness.as_str().to_string(),
                events: 0,
                tokens_in: 0,
                tokens_out: 0,
                cost_eur: 0.0,
            });
        h.events += 1;
        h.tokens_in += e.tokens_in;
        h.tokens_out += e.tokens_out;
        h.cost_eur += e.cost_eur;
    }

    s.sessions = sessions.len() as u64;
    s.unpriced_models = models
        .values()
        .filter(|m| m.unpriced)
        .map(|m| m.model.clone())
        .collect();
    s.credit_priced_models = credit_models.into_iter().collect();
    s.preview_priced_models = preview_models.into_iter().collect();
    s.unattributed_token_pct = if s.tokens_out == 0 {
        0.0
    } else {
        100.0 * unattributed_out as f64 / s.tokens_out as f64
    };

    // Rank breakdowns by cost desc, then tokens_out desc, for a stable, useful order.
    s.by_model = models.into_values().collect();
    s.by_model.sort_by(|a, b| {
        b.cost_eur
            .total_cmp(&a.cost_eur)
            .then(
                b.cost_credits
                    .unwrap_or(0.0)
                    .total_cmp(&a.cost_credits.unwrap_or(0.0)),
            )
            .then(b.tokens_out.cmp(&a.tokens_out))
            .then(a.model.cmp(&b.model))
    });
    s.by_agent = agents.into_values().collect();
    s.by_agent.sort_by(|a, b| {
        b.cost_eur
            .total_cmp(&a.cost_eur)
            .then(b.tokens_out.cmp(&a.tokens_out))
            .then(a.agent.cmp(&b.agent))
    });
    s.by_harness = harnesses.into_values().collect();
    s.by_harness.sort_by(|a, b| {
        b.cost_eur
            .total_cmp(&a.cost_eur)
            .then(a.harness.cmp(&b.harness))
    });

    s
}

/// Sum the cost of events at or after `since_ms` — used for today / this-week spend.
pub fn windowed_cost(events: &[Event], since_ms: i64) -> f64 {
    events
        .iter()
        .filter(|e| e.ts >= since_ms)
        .map(|e| e.cost_eur)
        .sum()
}

#[cfg(test)]
mod tests {
    use crate::model::{Event, Harness, PricingKind};

    use super::build_summary;

    fn event(
        model: &str,
        pricing_kind: PricingKind,
        tokens_in: u64,
        tokens_out: u64,
        cost_eur: f64,
        cost_credits: Option<f64>,
    ) -> Event {
        Event {
            id: format!("{model}-{tokens_in}-{tokens_out}-{}", pricing_kind.as_str()),
            ts: 0,
            harness: Harness::Codex,
            project: "/repo".to_string(),
            agent: "main".to_string(),
            is_subagent: false,
            model: model.to_string(),
            session_id: "s".to_string(),
            tokens_in,
            tokens_out,
            cache_read: 0,
            cache_write_5m: 0,
            cache_write_1h: 0,
            duration_ms: None,
            loc_added: 0,
            loc_removed: 0,
            loc_failed: false,
            skills: Vec::new(),
            cost_eur,
            cost_credits,
            pricing_kind,
            unpriced: false,
        }
    }

    #[test]
    fn bucketed_model_output_totals_track_pricing_mode() {
        let events = vec![
            event(
                "gpt-5.4",
                PricingKind::ChatgptIncluded,
                1000,
                500,
                0.0,
                Some(12.5),
            ),
            event("gpt-5.4", PricingKind::ApiMoney, 1000, 200, 4.0, None),
            event(
                "gpt-5.3-codex-spark",
                PricingKind::ChatgptPreview,
                1000,
                300,
                0.0,
                None,
            ),
            event("llama-local", PricingKind::LocalFree, 1000, 250, 0.0, None),
            event("gpt-5.5", PricingKind::ReportedCost, 1000, 1000, 8.0, None),
        ];

        let s = build_summary(&events);
        let gpt54 = s.by_model.iter().find(|m| m.model == "gpt-5.4").unwrap();
        assert_eq!(gpt54.money_tokens_out, 200);
        assert_eq!(gpt54.credit_tokens_out, 500);

        let spark = s
            .by_model
            .iter()
            .find(|m| m.model == "gpt-5.3-codex-spark")
            .unwrap();
        assert_eq!(spark.money_tokens_out, 0);
        assert_eq!(spark.credit_tokens_out, 0);

        let local = s
            .by_model
            .iter()
            .find(|m| m.model == "llama-local")
            .unwrap();
        assert_eq!(local.money_tokens_out, 0);
        assert_eq!(local.credit_tokens_out, 0);

        let gpt55 = s.by_model.iter().find(|m| m.model == "gpt-5.5").unwrap();
        assert_eq!(gpt55.money_tokens_out, 1000);
        assert_eq!(gpt55.credit_tokens_out, 0);
    }
}
