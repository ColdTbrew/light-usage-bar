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

pub(crate) fn windows(provider: &ProviderDisplay, expanded: bool) -> &[DisplayWindow] {
    let windows = provider
        .last_success
        .as_ref()
        .map(|value| value.windows.as_slice())
        .unwrap_or_default();
    &windows[..if expanded {
        windows.len()
    } else {
        windows.len().min(2)
    }]
}

pub(crate) fn details(provider: &ProviderDisplay) -> Vec<String> {
    let mut lines = vec![format!(
        "Next refresh: {}",
        provider
            .next_refresh_ms
            .map(date)
            .unwrap_or_else(|| "Not scheduled".into())
    )];
    if let Some(value) = &provider.last_success {
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
        lines.push(format!("Source: {source} · {provenance}"));
        for (index, window) in value.windows.iter().enumerate() {
            lines.push(format!(
                "Window {} · ID: {}",
                index + 1,
                window.id.as_deref().unwrap_or("Unknown")
            ));
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
        let observation = provider.last_success.as_mut().unwrap();
        observation.windows = vec![observation.windows[0].clone(); 5];
        assert_eq!(freshness(provider), "Stale reading");
        assert_eq!(fetch(provider.fetch), "Sign-in required");
        assert_eq!(windows(provider, false).len(), 2);
        assert_eq!(windows(provider, true).len(), 5);
        assert!(matches!(
            measurement(&windows(provider, false)[0].measurement),
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
