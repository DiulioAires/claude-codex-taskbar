use super::*;

fn model_usage() -> ProviderUsage {
    ProviderUsage {
        five_hour: Some(WindowUsage {
            used_pct: 18.0,
            resets_at: None,
        }),
        weekly: Some(WindowUsage {
            used_pct: 3.0,
            resets_at: None,
        }),
        weekly_by_model: vec![
            (
                ModelKind::Opus,
                WindowUsage {
                    used_pct: 72.0,
                    resets_at: Some(1793948399),
                },
            ),
            (
                ModelKind::Sonnet,
                WindowUsage {
                    used_pct: 0.0,
                    resets_at: None,
                },
            ),
        ],
        extra_usage_enabled: true,
    }
}

fn claude_tip(lang: Lang, status: &ProviderStatus) -> String {
    tooltip_text(
        lang,
        status,
        &ProviderStatus::Disabled,
        &[],
        TaskbarHints::default(),
        1793948399 - 4320,
        false,
    )
}

#[test]
fn model_tooltip_shows_weekly_limits_and_localized_reset() {
    let status = ProviderStatus::Ok {
        usage: model_usage(),
        at: 1000,
    };
    for (lang, opus, sonnet, extra, countdown) in [
        (
            Lang::En,
            "Opus wk",
            "Sonnet wk",
            "Extra usage enabled",
            "in 1h 12m",
        ),
        (
            Lang::PtBr,
            "Opus sem",
            "Sonnet sem",
            "Uso extra habilitado",
            "em 1h 12min",
        ),
    ] {
        let tip = claude_tip(lang, &status);
        let when = format_local_day_hhmm(1793948399);
        let reset = fmt(t(lang, Key::ResetsFmt), &when);
        assert!(
            tip.contains(&format!("\n  {opus}: 72% · {reset} ({countdown})\n")),
            "{tip}"
        );
        assert!(tip.contains(&format!("\n  {sonnet}: 0%\n")), "{tip}");
        assert!(tip.contains(&format!("\n  {extra}\n")), "{tip}");
    }
}

#[test]
fn model_tooltip_omits_absent_model_and_disabled_extra_usage() {
    let mut usage = model_usage();
    usage.weekly_by_model.remove(0);
    usage.extra_usage_enabled = false;
    let status = ProviderStatus::Ok { usage, at: 1000 };
    let tip = claude_tip(Lang::En, &status);
    assert!(!tip.contains("Opus"));
    assert!(tip.contains("Sonnet wk: 0%"));
    assert!(!tip.contains("Extra usage"));
}

#[test]
fn model_tooltip_omits_all_unavailable_details() {
    let usage = ProviderUsage::default();
    let status = ProviderStatus::Ok { usage, at: 1000 };
    let tip = claude_tip(Lang::En, &status);
    assert!(!tip.contains("Opus"));
    assert!(!tip.contains("Sonnet"));
    assert!(!tip.contains("Extra usage"));
}

#[test]
fn model_tooltip_preserves_stale_notice() {
    let status = ProviderStatus::Stale {
        usage: model_usage(),
        at: 1000,
        err: FetchError::Network("timeout".into()),
    };
    let tip = claude_tip(Lang::En, &status);
    assert!(tip.contains("Opus wk: 72%"));
    assert!(tip.contains("Sonnet wk: 0%"));
    assert!(tip.contains("Update failed (timeout); last data at"));
}

#[test]
fn model_tooltip_never_invents_details_without_readings() {
    for status in [
        ProviderStatus::Loading,
        ProviderStatus::Disabled,
        ProviderStatus::Failed(FetchError::Expired),
    ] {
        let tip = claude_tip(Lang::En, &status);
        assert!(!tip.contains("Opus"));
        assert!(!tip.contains("Sonnet"));
        assert!(!tip.contains("Extra usage"));
    }
}

#[test]
fn model_data_does_not_change_strip_items() {
    let status = ProviderStatus::Ok {
        usage: model_usage(),
        at: 1000,
    };
    let items = build_items(
        &Config::default(),
        &HwSnapshot::default(),
        &status,
        &ProviderStatus::Disabled,
        "P",
        Lang::En,
    );
    let limits: Vec<_> = items
        .iter()
        .filter(|item| item.kind == Kind::Claude)
        .map(|item| (item.id.clone(), item.value))
        .collect();
    assert_eq!(
        limits,
        vec![
            (ItemId::ClaudeFive, Some(18.0)),
            (ItemId::ClaudeWeek, Some(3.0))
        ]
    );
}

#[test]
fn remaining_model_and_cached_values_keep_their_meaning() {
    for (lang, opus, sonnet) in [
        (Lang::En, "Opus wk: 28% left", "Sonnet wk: 100% left"),
        (
            Lang::PtBr,
            "Opus sem: 28% restante",
            "Sonnet sem: 100% restante",
        ),
    ] {
        let status = ProviderStatus::Stale {
            usage: model_usage(),
            at: 1000,
            err: FetchError::Network("timeout".into()),
        };
        let tip = tooltip_text(
            lang,
            &status,
            &ProviderStatus::Disabled,
            &[],
            TaskbarHints::default(),
            1000,
            true,
        );
        assert!(tip.contains(opus), "{tip}");
        assert!(tip.contains(sonnet), "{tip}");
        assert!(tip.contains("timeout"), "{tip}");
        let cfg = Config {
            show_remaining: true,
            ..Config::default()
        };
        let items = build_items(
            &cfg,
            &HwSnapshot::default(),
            &status,
            &ProviderStatus::Disabled,
            "",
            lang,
        );
        let item = items.iter().find(|i| i.id == ItemId::ClaudeFive).unwrap();
        assert_eq!(item.text(), "82%");
        assert!(item.dim);
        assert_eq!(item.value, Some(18.0));
    }
}
