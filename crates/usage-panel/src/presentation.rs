use usage_display::*;

#[derive(Debug, PartialEq)]
pub(crate) enum Measurement {
    Unknown,
    Known { text: String, fraction: Option<f32> },
}

pub(crate) fn measurement(value: &DisplayMeasurement) -> Measurement {
    let direction = |value| match value {
        DisplayDirection::Used => "used",
        DisplayDirection::Remaining => "remaining",
    };
    match value {
        DisplayMeasurement::Unknown {} => Measurement::Unknown,
        DisplayMeasurement::Percent {
            value,
            direction: d,
        } => Measurement::Known {
            text: format!("{value}% {}", direction(*d)),
            fraction: Some((value / 100.).clamp(0., 1.) as f32),
        },
        DisplayMeasurement::Amount {
            value,
            limit,
            direction: d,
            unit,
        } => Measurement::Known {
            text: match limit {
                Some(limit) => format!("{value} / {limit} {unit} {}", direction(*d)),
                None => format!("{value} {unit} {} · Limit unknown", direction(*d)),
            },
            fraction: None,
        },
    }
}

pub(crate) fn freshness(provider: &ProviderDisplay) -> &'static str {
    match (
        provider.enabled,
        provider.last_success.is_some(),
        provider.freshness,
    ) {
        (false, true, DisplayFreshness::Stale) => "Paused · Saved reading · Stale",
        (false, true, _) => "Paused · Saved reading",
        (false, false, _) => "Paused · No reading",
        (_, _, DisplayFreshness::Stale) => "Stale reading",
        (_, _, DisplayFreshness::Missing) => "No reading",
        _ => "Up to date",
    }
}

pub(crate) fn fetch(value: DisplayFetch) -> &'static str {
    match value {
        DisplayFetch::Idle => "",
        DisplayFetch::Refreshing => "Refreshing…",
        DisplayFetch::NeedsAuthentication => "Sign-in required",
        DisplayFetch::PermissionDenied => "Permission needed",
        DisplayFetch::RateLimited => "Refresh limited",
        DisplayFetch::TemporarilyUnavailable => "Temporarily unavailable",
        DisplayFetch::Unsupported => "Usage source unavailable",
        DisplayFetch::NeedsVerification => "Verification required",
        DisplayFetch::InvalidResponse => "Could not read usage",
    }
}

pub(crate) fn date(value: i64) -> String {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|date| date.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| format!("Timestamp {value} ms UTC"))
}

pub(crate) fn reset(value: Option<i64>) -> String {
    value
        .map(|value| format!("Resets {}", date(value)))
        .unwrap_or_else(|| "Reset time unknown".into())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum QuotaPeriod {
    FiveHour,
    SevenDay,
}

impl QuotaPeriod {
    fn claude_id(self) -> &'static str {
        match self {
            Self::FiveHour => "five_hour",
            Self::SevenDay => "seven_day",
        }
    }

    fn from_seconds(seconds: Option<u64>) -> Option<Self> {
        match seconds {
            Some(18000) => Some(Self::FiveHour),
            Some(604800) => Some(Self::SevenDay),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::FiveHour => "5h",
            Self::SevenDay => "7d",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum QuotaRow<'a> {
    Reported {
        window: &'a DisplayWindow,
        period: Option<QuotaPeriod>,
    },
    Missing(QuotaPeriod),
}

impl QuotaRow<'_> {
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Missing(period)
            | Self::Reported {
                period: Some(period),
                ..
            } => period.label(),
            Self::Reported { window, .. } => window
                .label
                .as_deref()
                .or(window.id.as_deref())
                .unwrap_or("Quota window"),
        }
    }
}

pub(crate) fn quota_rows(provider: &ProviderDisplay) -> Vec<QuotaRow<'_>> {
    let windows = provider
        .last_success
        .as_ref()
        .map(|value| value.windows.as_slice())
        .unwrap_or_default();
    if provider.provider != DisplayProvider::Claude {
        return windows
            .iter()
            .map(|window| QuotaRow::Reported {
                window,
                period: if provider.provider == DisplayProvider::Codex
                    && window.model.is_none()
                    && matches!(
                        window.id.as_deref(),
                        Some("primary_window" | "secondary_window")
                    ) {
                    QuotaPeriod::from_seconds(window.period_seconds)
                } else {
                    None
                },
            })
            .collect();
    }
    let mut selected = Vec::with_capacity(2);
    let mut rows = Vec::with_capacity(windows.len() + 2);
    for period in [QuotaPeriod::FiveHour, QuotaPeriod::SevenDay] {
        if let Some(index) = windows
            .iter()
            .position(|window| window.id.as_deref() == Some(period.claude_id()))
        {
            selected.push(index);
            rows.push(QuotaRow::Reported {
                window: &windows[index],
                period: Some(period),
            });
        } else {
            rows.push(QuotaRow::Missing(period));
        }
    }
    rows.extend(
        windows
            .iter()
            .enumerate()
            .filter(|(index, _)| !selected.contains(index))
            .map(|(_, window)| QuotaRow::Reported {
                window,
                period: None,
            }),
    );
    rows
}

pub(crate) fn details(provider: &ProviderDisplay, demo: bool) -> Vec<String> {
    let mut lines = vec![format!(
        "Next refresh: {}",
        provider
            .next_refresh_ms
            .map(date)
            .unwrap_or_else(|| "Not scheduled".into())
    )];
    if let Some(value) = &provider.last_success {
        if value.account_binding == DisplayAccountBinding::Observation {
            lines.push("Account not identified for this reading".into());
        }
        lines.push(format!("Retrieved: {}", date(value.fetched_at_ms)));
        lines.push(format!(
            "Observed: {}",
            value
                .observed_at_ms
                .map(date)
                .unwrap_or_else(|| "Unknown".into())
        ));
        lines.push(format!("Expires: {}", date(value.expires_at_ms)));
        let source = match value.source.kind {
            DisplaySourceKind::DocumentedApi => "Documented API",
            DisplaySourceKind::ProviderClient => "Provider client",
            DisplaySourceKind::LocalLog => "Local log",
            DisplaySourceKind::LocalService => "Local service",
            DisplaySourceKind::CommandLine => "Command line",
        };
        let provenance = match value.source.provenance {
            DisplayProvenance::ProviderReported => "Provider reported",
            DisplayProvenance::LocalObserved => "Local observed",
            DisplayProvenance::Derived => "Derived",
        };
        lines.push(if demo {
            "Source: Synthetic demo data".into()
        } else {
            format!("Source: {source} · {provenance}")
        });
        for (index, window) in value.windows.iter().enumerate() {
            lines.push(format!(
                "Window {} · ID: {}",
                index + 1,
                window.id.as_deref().unwrap_or("Unknown")
            ));
            if let Some(label) = &window.label {
                lines.push(format!("Label: {label}"));
            }
            if let Some(model) = &window.model {
                lines.push(format!("Model: {model}"));
            }
            if let Some(period) = window.period_seconds {
                lines.push(format!("Duration: {period} seconds"));
            }
            if let Some(start) = window.period_started_at_ms {
                lines.push(format!("Started: {}", date(start)));
            }
            if let Some(expiry) = window.trial_expires_at_ms {
                lines.push(format!("Trial expires: {}", date(expiry)));
            }
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(id: Option<&str>, label: &str, seconds: Option<u64>) -> DisplayWindow {
        DisplayWindow {
            id: id.map(str::to_owned),
            label: Some(label.into()),
            model: None,
            measurement: DisplayMeasurement::Percent {
                value: 42.,
                direction: DisplayDirection::Used,
            },
            period_seconds: seconds,
            period_started_at_ms: None,
            resets_at_ms: Some(0),
            trial_expires_at_ms: None,
        }
    }

    fn provider(windows: Vec<DisplayWindow>) -> ProviderDisplay {
        ProviderDisplay {
            provider: DisplayProvider::Claude,
            enabled: true,
            last_success: Some(DisplayObservation {
                account_binding: DisplayAccountBinding::Keyed,
                windows,
                observed_at_ms: Some(0),
                fetched_at_ms: 0,
                expires_at_ms: 60000,
                source: DisplaySource {
                    kind: DisplaySourceKind::ProviderClient,
                    provenance: DisplayProvenance::ProviderReported,
                },
            }),
            freshness: DisplayFreshness::Fresh,
            fetch: DisplayFetch::Idle,
            next_refresh_ms: None,
        }
    }

    fn reported<'a>(row: &QuotaRow<'a>) -> &'a DisplayWindow {
        match row {
            QuotaRow::Reported { window, .. } => window,
            QuotaRow::Missing(_) => panic!("expected a reported quota"),
        }
    }

    #[test]
    fn claude_shuffled_quotas_lead_and_duplicates_and_scoped_extras_survive() {
        let mut scoped = window(Some("seven_day_sonnet"), "Sonnet weekly", Some(604800));
        scoped.model = Some("Sonnet".into());
        let mut weekly = window(Some("seven_day"), "Weekly limit", Some(604800));
        weekly.measurement = DisplayMeasurement::Percent {
            value: 73.,
            direction: DisplayDirection::Remaining,
        };
        weekly.resets_at_ms = Some(604800000);
        let windows = vec![
            scoped,
            weekly,
            window(Some("five_hour"), "Session limit", Some(18000)),
            window(Some("five_hour"), "Duplicate session", Some(18000)),
            window(None, "Future quota", Some(604800)),
        ];
        let provider = provider(windows.clone());
        let rows = quota_rows(&provider);
        assert_eq!(
            rows.iter().map(QuotaRow::label).collect::<Vec<_>>(),
            [
                "5h",
                "7d",
                "Sonnet weekly",
                "Duplicate session",
                "Future quota"
            ]
        );
        assert_eq!(reported(&rows[0]), &windows[2]);
        assert_eq!(reported(&rows[1]), &windows[1]);
        assert_eq!(reported(&rows[2]), &windows[0]);
        assert_eq!(reported(&rows[3]), &windows[3]);
        assert_eq!(reported(&rows[4]), &windows[4]);
    }

    #[test]
    fn claude_missing_slots_never_borrow_scoped_or_duration_only_readings() {
        let provider = provider(vec![
            window(Some("seven_day_opus"), "Opus weekly", Some(604800)),
            window(Some("future_weekly"), "Future weekly", Some(604800)),
            window(None, "Unnamed session", Some(18000)),
        ]);
        let rows = quota_rows(&provider);
        assert!(matches!(rows[0], QuotaRow::Missing(QuotaPeriod::FiveHour)));
        assert!(matches!(rows[1], QuotaRow::Missing(QuotaPeriod::SevenDay)));
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[2].label(), "Opus weekly");
        assert_eq!(rows[3].label(), "Future weekly");
        assert_eq!(rows[4].label(), "Unnamed session");
    }

    #[test]
    fn claude_empty_and_absent_observations_show_both_unavailable_slots_and_status() {
        let mut provider = provider(vec![]);
        provider.fetch = DisplayFetch::NeedsVerification;
        for absent in [false, true] {
            if absent {
                provider.last_success = None;
            }
            let rows = quota_rows(&provider);
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[0], QuotaRow::Missing(QuotaPeriod::FiveHour)));
            assert!(matches!(rows[1], QuotaRow::Missing(QuotaPeriod::SevenDay)));
            assert_eq!(fetch(provider.fetch), "Verification required");
        }
    }

    #[test]
    fn claude_session_only_keeps_weekly_unavailable() {
        let session = window(Some("five_hour"), "Session", Some(18000));
        let provider = provider(vec![session.clone()]);
        let rows = quota_rows(&provider);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label(), "5h");
        assert_eq!(reported(&rows[0]), &session);
        assert!(matches!(rows[1], QuotaRow::Missing(QuotaPeriod::SevenDay)));
        assert_eq!(rows[1].label(), "7d");
    }

    #[test]
    fn claude_unknown_weekly_measurement_is_preserved_without_a_meter() {
        let mut weekly = window(Some("seven_day"), "Weekly limit", Some(604800));
        weekly.measurement = DisplayMeasurement::Unknown {};
        weekly.resets_at_ms = None;
        let provider = provider(vec![weekly.clone()]);
        let rows = quota_rows(&provider);
        assert!(matches!(rows[0], QuotaRow::Missing(QuotaPeriod::FiveHour)));
        assert_eq!(rows[1].label(), "7d");
        assert_eq!(reported(&rows[1]), &weekly);
        assert_eq!(
            measurement(&reported(&rows[1]).measurement),
            Measurement::Unknown
        );
    }

    #[test]
    fn codex_titles_follow_reported_durations_and_preserve_extra_and_model_labels() {
        let mut model = window(Some("primary_window"), "Model specific", Some(18000));
        model.model = Some("model-name".into());
        let mut provider = provider(vec![
            window(Some("primary_window"), "Primary", Some(604800)),
            window(Some("secondary_window"), "Secondary", Some(18000)),
            window(Some("primary_window"), "Unknown duration", None),
            window(Some("secondary_window"), "Other duration", Some(3600)),
            window(Some("extra"), "Extra weekly", Some(604800)),
            model,
        ]);
        provider.provider = DisplayProvider::Codex;
        let rows = quota_rows(&provider);
        assert_eq!(
            rows.iter().map(QuotaRow::label).collect::<Vec<_>>(),
            [
                "7d",
                "5h",
                "Unknown duration",
                "Other duration",
                "Extra weekly",
                "Model specific"
            ]
        );
        for (row, window) in rows
            .iter()
            .zip(&provider.last_success.as_ref().unwrap().windows)
        {
            assert_eq!(reported(row), window);
        }
        provider.provider = DisplayProvider::Antigravity;
        assert_eq!(
            quota_rows(&provider)
                .iter()
                .map(QuotaRow::label)
                .collect::<Vec<_>>(),
            [
                "Primary",
                "Secondary",
                "Unknown duration",
                "Other duration",
                "Extra weekly",
                "Model specific"
            ]
        );
    }

    #[test]
    fn observation_details_disclose_missing_account_identification() {
        let mut provider = provider(vec![]);
        let keyed = details(&provider, false);
        assert!(!keyed.iter().any(|line| line.contains("verified")));
        assert!(
            !keyed
                .iter()
                .any(|line| line.contains("Account not identified"))
        );
        provider.last_success.as_mut().unwrap().account_binding =
            DisplayAccountBinding::Observation;
        assert!(
            details(&provider, false)
                .iter()
                .any(|line| line == "Account not identified for this reading")
        );
    }

    #[test]
    fn details_preserve_source_identity_and_explicitly_mark_synthetic_readings() {
        let provider = provider(vec![window(
            Some("five_hour"),
            "Original session label",
            Some(18000),
        )]);
        let real = details(&provider, false);
        assert!(real.iter().any(|line| line == "Window 1 · ID: five_hour"));
        assert!(
            real.iter()
                .any(|line| line == "Label: Original session label")
        );
        assert!(
            real.iter()
                .any(|line| line == "Source: Provider client · Provider reported")
        );
        let demo = details(&provider, true);
        assert!(
            demo.iter()
                .any(|line| line == "Source: Synthetic demo data")
        );
        assert!(!demo.iter().any(|line| line.contains("Provider reported")));
    }

    #[test]
    fn percentages_preserve_direction_and_overage_only_clamps_drawing() {
        assert_eq!(
            measurement(&DisplayMeasurement::Percent {
                value: 142.,
                direction: DisplayDirection::Used
            }),
            Measurement::Known {
                text: "142% used".into(),
                fraction: Some(1.)
            }
        );
        assert_eq!(
            measurement(&DisplayMeasurement::Percent {
                value: 28.,
                direction: DisplayDirection::Remaining
            }),
            Measurement::Known {
                text: "28% remaining".into(),
                fraction: Some(0.28)
            }
        );
        assert_eq!(
            measurement(&DisplayMeasurement::Unknown {}),
            Measurement::Unknown
        );
    }

    #[test]
    fn amounts_keep_units_and_zero_or_unknown_limits_without_inventing_a_percentage() {
        for (limit, expected) in [
            (Some(0.), "12 / 0 credits remaining"),
            (None, "12 credits remaining · Limit unknown"),
        ] {
            assert_eq!(
                measurement(&DisplayMeasurement::Amount {
                    value: 12.,
                    limit,
                    unit: "credits".into(),
                    direction: DisplayDirection::Remaining
                }),
                Measurement::Known {
                    text: expected.into(),
                    fraction: None
                }
            );
        }
    }

    #[test]
    fn failed_refresh_keeps_stale_reading_and_all_windows_available() {
        let mut snapshot = demo_snapshot();
        let provider = &mut snapshot.providers[0];
        provider.fetch = DisplayFetch::NeedsAuthentication;
        provider.freshness = DisplayFreshness::Stale;
        let observation = provider.last_success.as_mut().unwrap();
        observation.windows = vec![
            window(Some("five_hour"), "Session", Some(18000)),
            window(Some("seven_day"), "Weekly", Some(604800)),
            window(Some("extra_one"), "One", None),
            window(Some("extra_two"), "Two", None),
            window(Some("extra_three"), "Three", None),
        ];
        assert_eq!(freshness(provider), "Stale reading");
        assert_eq!(fetch(provider.fetch), "Sign-in required");
        let rows = quota_rows(provider);
        assert_eq!(rows.len(), 5);
        assert_eq!(
            rows[..2].iter().map(QuotaRow::label).collect::<Vec<_>>(),
            ["5h", "7d"]
        );
        assert!(matches!(
            measurement(&reported(&rows[0]).measurement),
            Measurement::Known {
                fraction: Some(_),
                ..
            }
        ));
        provider.enabled = false;
        assert!(freshness(provider).contains("Saved reading"));
    }

    #[test]
    fn absolute_resets_do_not_claim_a_future_countdown_for_old_readings() {
        assert_eq!(reset(Some(0)), "Resets 1970-01-01 00:00 UTC");
        assert_eq!(reset(None), "Reset time unknown");
    }
}
