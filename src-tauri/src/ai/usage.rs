use super::*;
use serde_json::Value;
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedUsage {
    pub warnings: Vec<String>,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub cache: Option<u64>,
    pub reported: Option<f64>,
    pub estimated: Option<f64>,
    pub conservative: bool,
    pub billing_source: Option<String>,
    pub provider_estimated: Option<bool>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUsage {
    pub known_reported_subtotal: f64,
    pub known_estimated_subtotal: f64,
    pub unknown_attempt_count: u64,
    pub input: Option<u64>,
    pub output: Option<u64>,
    pub complete: bool,
}
pub fn normalize_usage(v: &Value, tariff: Option<&Tariff>, model: &str) -> NormalizedUsage {
    let mut u = NormalizedUsage {
        input: v["prompt_tokens"].as_u64(),
        output: v["completion_tokens"].as_u64(),
        cache: v["prompt_tokens_details"]["cached_tokens"].as_u64(),
        reported: v["cost"].as_f64().filter(|n| n.is_finite() && *n >= 0.),
        billing_source: v["opencode"]["billing_source"].as_str().map(str::to_owned),
        provider_estimated: v["opencode"]["estimated"].as_bool(),
        ..Default::default()
    };
    if u.cache.zip(u.input).is_some_and(|(c, i)| c > i) {
        u.cache = None;
        u.warnings
            .push("Cache devolvida superior à entrada; consumo de cache desconhecido.".into());
    }
    if let (Some(i), Some(o), Some(t)) = (u.input, u.output, tariff.filter(|t| t.model == model)) {
        let (c, rate) = match (u.cache, t.cached_input_per_million) {
            (Some(c), Some(r)) => (c, r),
            _ => (0, t.input_per_million),
        };
        u.conservative = u.cache.is_none() || t.cached_input_per_million.is_none();
        u.estimated = Some(
            ((i - c) as f64 * t.input_per_million
                + c as f64 * rate
                + o as f64 * t.output_per_million)
                / 1e6,
        );
    }
    u
}
pub fn aggregate_usage(values: &[NormalizedUsage]) -> SessionUsage {
    let mut s = SessionUsage {
        complete: true,
        ..Default::default()
    };
    let (mut i, mut o) = (0u64, 0u64);
    let (mut ik, mut ok) = (true, true);
    for v in values {
        if let Some(n) = v.reported {
            s.known_reported_subtotal += n;
        } else if let Some(n) = v.estimated {
            s.known_estimated_subtotal += n;
        } else {
            s.unknown_attempt_count += 1;
            s.complete = false;
        }
        if let Some(n) = v.input {
            i = i.saturating_add(n);
        } else {
            ik = false;
        }
        if let Some(n) = v.output {
            o = o.saturating_add(n);
        } else {
            ok = false;
        }
    }
    s.input = ik.then_some(i);
    s.output = ok.then_some(o);
    s
}
