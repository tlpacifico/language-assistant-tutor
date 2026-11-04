use language_assistant_tutor::ai::*;
use serde_json::json;
fn reference_tariff() -> Tariff {
    Tariff {
        model: "deepseek-v4.1-flash".into(),
        currency: "USD".into(),
        input_per_million: 0.30,
        output_per_million: 1.20,
        cached_input_per_million: Some(0.006),
        observed_at: "2026-11-02".into(),
        source_url: "https://opencode.ai/v2/docs/console/models/".into(),
    }
}
#[test]
fn cache_formula_and_unknown_usage() {
    let settings = AiSettingsView::default();
    let u = normalize_usage(
        &json!({"prompt_tokens":1000,"completion_tokens":100,"prompt_tokens_details":{"cached_tokens":200}}),
        Some(&reference_tariff()),
        &settings.model,
    );
    assert!((u.estimated.unwrap() - 0.0003612).abs() < 1e-12);
    let missing = normalize_usage(&json!({}), settings.tariff.as_ref(), &settings.model);
    assert_eq!(missing.input, None);
    assert_eq!(missing.estimated, None);
    let total = aggregate_usage(&[u, missing]);
    assert_eq!(total.unknown_attempt_count, 1);
    assert!(!total.complete);
    assert_eq!(total.input, None);
}
#[test]
fn explicit_zero_is_known_and_model_mismatch_has_no_estimate() {
    let s = AiSettingsView::default();
    let u = normalize_usage(
        &json!({"prompt_tokens":0,"completion_tokens":0,"cost":0}),
        None,
        &s.model,
    );
    assert_eq!(u.reported, Some(0.));
    assert_eq!(aggregate_usage(&[u]).unknown_attempt_count, 0);
    assert_eq!(
        normalize_usage(
            &json!({"prompt_tokens":1,"completion_tokens":1}),
            Some(&reference_tariff()),
            "other"
        )
        .estimated,
        None
    );
}
#[test]
fn invalid_cache_cannot_make_negative_estimate() {
    let s = AiSettingsView::default();
    let u = normalize_usage(
        &json!({"prompt_tokens":1,"completion_tokens":1,"prompt_tokens_details":{"cached_tokens":5}}),
        Some(&reference_tariff()),
        &s.model,
    );
    assert_eq!(u.cache, None);
    assert!(u.conservative);
    assert!(u.estimated.unwrap() > 0.);
}
