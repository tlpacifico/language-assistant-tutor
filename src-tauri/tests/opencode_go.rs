use language_assistant_tutor::{ai::*, db::Database};

fn legacy(base: &str) -> AiSettingsView {
    AiSettingsView {
        version: 7,
        base_url: base.into(),
        model: "deepseek-v4.1-flash".into(),
        tariff: Some(Tariff {
            model: "deepseek-v4.1-flash".into(),
            currency: "USD".into(),
            input_per_million: 0.30,
            output_per_million: 1.20,
            cached_input_per_million: Some(0.006),
            observed_at: "2026-11-02".into(),
            source_url: "https://opencode.ai/v2/docs/console/models/".into(),
        }),
        credential_configured: false,
        consent: Some(Consent {
            destination: base.into(),
            notice_version: "text-external-v1".into(),
            accepted_at: "now".into(),
        }),
    }
}

fn seed(db: &Database, settings: &AiSettingsView) {
    db.with_connection(|c| {
        c.execute(
            "INSERT INTO ai_settings VALUES(1,?1,?2)",
            rusqlite::params![
                settings.version as i64,
                serde_json::to_string(settings).unwrap()
            ],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
}

#[test]
fn default_go_does_not_invent_per_request_subscription_prices() {
    let settings = AiSettingsView::default();
    assert_eq!(
        settings.base_url,
        "https://opencode.ai/inference/go/openai/v1"
    );
    assert_eq!(settings.model, "deepseek-v4.1-flash");
    assert!(settings.tariff.is_none());
}

#[test]
fn reference_settings_move_once_to_go_with_a_new_consent_and_preserved_history() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sqlite");
    let db = Database::open(path.clone()).unwrap();
    seed(&db, &legacy("https://opencode.ai/inference/openai/v1"));
    db.with_connection(|c| {
        c.execute(
            "INSERT INTO ai_receipts VALUES('historical-command','test','old-hash',NULL)",
            [],
        )
        .unwrap();
        Ok(())
    })
    .unwrap();
    let updated = db.ai_settings().unwrap();
    assert_eq!(
        updated.base_url,
        "https://opencode.ai/inference/go/openai/v1"
    );
    assert_eq!(updated.version, 8);
    assert!(updated.consent.is_none());
    assert!(updated.tariff.is_none());
    assert_eq!(db.ai_settings().unwrap().version, 8);
    assert!(db
        .ai_receipt_matches("historical-command", "test", "old-hash")
        .unwrap());
    drop(db);
    let db = Database::open(path).unwrap();
    assert_eq!(db.ai_settings().unwrap().version, 8);
    let mut chosen = updated;
    chosen.version += 1;
    chosen.base_url = "https://opencode.ai/inference/openai/v1".into();
    db.put_ai_settings(8, &chosen, ("explicit-choice", "settings", "hash"))
        .unwrap();
    assert_eq!(db.ai_settings().unwrap().base_url, chosen.base_url);
}

#[test]
fn custom_settings_and_other_models_are_preserved() {
    for (base, model) in [
        ("https://example.com/api", "deepseek-v4.1-flash"),
        ("https://opencode.ai/inference/openai/v1", "another-model"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let db = Database::open(dir.path().join("t.sqlite")).unwrap();
        let mut settings = legacy(base);
        settings.model = model.into();
        seed(&db, &settings);
        assert_eq!(db.ai_settings().unwrap(), settings);
    }
}
