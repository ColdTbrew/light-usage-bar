use serde_json::Value;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use usage_core::*;

fn json(bytes: &[u8]) -> Result<Value, FetchError> {
    serde_json::from_slice(bytes).map_err(|_| FetchError::InvalidResponse)
}
fn text(v: &Value) -> Option<String> {
    v.as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}
fn percent(v: &Value, direction: Direction) -> Measurement {
    v.as_f64()
        .and_then(|n| NonNegative::new(n).ok())
        .map(|value| Measurement::Percent { value, direction })
        .unwrap_or(Measurement::Unknown)
}
fn unix(v: &Value, millis: bool) -> Option<SystemTime> {
    let n = v.as_u64()?;
    UNIX_EPOCH.checked_add(if millis {
        Duration::from_millis(n)
    } else {
        Duration::from_secs(n)
    })
}
fn iso(v: &Value) -> Option<SystemTime> {
    let date = chrono::DateTime::parse_from_rfc3339(v.as_str()?).ok()?;
    let millis = u64::try_from(date.timestamp_millis()).ok()?;
    UNIX_EPOCH.checked_add(Duration::from_millis(millis))
}
fn window(id: &str) -> QuotaWindow {
    QuotaWindow {
        id: WindowId::new(id).ok(),
        label: None,
        measurement: Measurement::Unknown,
        model: None,
        period: None,
        period_started_at: None,
        resets_at: None,
        trial_expires_at: None,
    }
}
fn snapshot(
    identity: &Identity,
    windows: Vec<QuotaWindow>,
    now: SystemTime,
    derived: bool,
) -> Result<Snapshot, FetchError> {
    if windows.is_empty() {
        return Err(FetchError::InvalidResponse);
    }
    Ok(Snapshot {
        identity: identity.clone(),
        windows,
        timing: Timing {
            observed_at: None,
            fetched_at: now,
            expires_at: now.checked_add(Duration::from_secs(120)).unwrap_or(now),
        },
        source: Source {
            kind: if identity.provider == Provider::Antigravity {
                SourceKind::LocalService
            } else {
                SourceKind::ProviderClient
            },
            provenance: if derived {
                Provenance::Derived(
                    Derivation::new(
                        "Reset reconstructed from reset_after_seconds at retrieval time",
                    )
                    .unwrap(),
                )
            } else {
                Provenance::ProviderReported
            },
        },
    })
}

pub fn parse_claude(
    bytes: &[u8],
    identity: &Identity,
    now: SystemTime,
) -> Result<Snapshot, FetchError> {
    let v = json(bytes)?;
    let mut windows = Vec::new();
    for key in [
        "five_hour",
        "seven_day",
        "seven_day_opus",
        "seven_day_sonnet",
        "seven_day_oauth_apps",
    ] {
        if !v.get(key).is_some_and(Value::is_object) {
            continue;
        }
        let mut w = window(key);
        w.measurement = percent(&v[key]["utilization"], Direction::Used);
        w.period = Some(Duration::from_secs(if key == "five_hour" {
            18000
        } else {
            604800
        }));
        w.resets_at = iso(&v[key]["resets_at"]);
        w.model = key.strip_prefix("seven_day_").map(str::to_owned);
        windows.push(w);
    }
    if let Some(limits) = v["limits"].as_array() {
        for (index, limit) in limits.iter().enumerate() {
            if limit["group"] != "weekly" || limit["kind"] != "weekly_scoped" {
                continue;
            }
            let mut w = window(&format!("limits.{index}"));
            w.model = text(&limit["scope"]["model"]["id"]);
            if let Some(ref model) = w.model {
                w.id = WindowId::new(format!("weekly_scoped.{model}")).ok();
            }
            w.label = text(&limit["scope"]["model"]["display_name"]);
            w.measurement = percent(&limit["percent"], Direction::Used);
            w.resets_at = iso(&limit["resets_at"]);
            w.period = Some(Duration::from_secs(604800));
            windows.push(w);
        }
    }
    snapshot(identity, windows, now, false)
}

pub fn parse_codex(
    bytes: &[u8],
    identity: &Identity,
    now: SystemTime,
) -> Result<Snapshot, FetchError> {
    let v = json(bytes)?;
    for key in ["user_id", "account_id"] {
        if let Some(value) = v.get(key).filter(|v| !v.is_null())
            && text(value).is_none()
        {
            return Err(FetchError::InvalidResponse);
        }
    }
    if let Some(user) = text(&v["user_id"])
        && AccountId::new(user).ok().as_ref() != Some(&identity.account)
    {
        return Err(FetchError::NeedsVerification);
    }
    if let Some(account) = text(&v["account_id"])
        && WorkspaceId::new(account).ok().as_ref() != identity.workspace.as_ref()
    {
        return Err(FetchError::NeedsVerification);
    }
    let mut windows = Vec::new();
    let mut derived = false;
    let mut add = |rate: &Value, prefix: &str, model: Option<String>, label: Option<String>| {
        for key in ["primary_window", "secondary_window"] {
            let Some(raw) = rate.get(key).filter(|v| v.is_object()) else {
                continue;
            };
            let mut w = window(&format!("{prefix}{key}"));
            w.measurement = percent(&raw["used_percent"], Direction::Used);
            w.period = raw["limit_window_seconds"]
                .as_u64()
                .map(Duration::from_secs);
            w.resets_at = unix(&raw["reset_at"], false);
            if w.resets_at.is_none() {
                w.resets_at = raw["reset_after_seconds"]
                    .as_u64()
                    .and_then(|n| now.checked_add(Duration::from_secs(n)));
                derived |= w.resets_at.is_some();
            }
            w.model = model.clone();
            w.label = label.clone();
            windows.push(w);
        }
    };
    add(&v["rate_limit"], "", None, None);
    if let Some(extra) = v["additional_rate_limits"].as_array() {
        for (i, rate) in extra.iter().enumerate() {
            let model = text(&rate["metered_feature"]).or_else(|| text(&rate["normal_model_slug"]));
            let key = model.clone().unwrap_or_else(|| i.to_string());
            add(
                &rate["rate_limit"],
                &format!("additional.{key}."),
                model,
                text(&rate["limit_name"]),
            );
        }
    }
    snapshot(identity, windows, now, derived)
}

pub fn parse_grok(
    bytes: &[u8],
    identity: &Identity,
    now: SystemTime,
) -> Result<Snapshot, FetchError> {
    let v = json(bytes)?;
    if v["jsonrpc"] != "2.0" || v["id"] != 1 || !v["result"].is_object() {
        return Err(FetchError::InvalidResponse);
    }
    let r = &v["result"];
    if !r["manageUrl"].is_string() {
        return Err(FetchError::InvalidResponse);
    }
    for key in [
        "hasAvailableUsage",
        "hasNonZeroIncludedLimit",
        "includedLimitZero",
        "trial",
        "isTeamSeat",
        "onDemandEligible",
        "onDemandEnabled",
    ] {
        if !r[key].is_boolean() {
            return Err(FetchError::InvalidResponse);
        }
    }
    let mut w = window("bot.usage");
    w.period_started_at =
        Some(unix(&r["currentPeriodStartMs"], true).ok_or(FetchError::InvalidResponse)?);
    w.measurement = percent(&r["usagePercent"], Direction::Used);
    w.label = text(&r["planLabel"]).or_else(|| text(&r["fundingPlan"]));
    if r["trial"] == true {
        w.trial_expires_at = unix(&r["nextResetAtMs"], true);
    } else {
        w.resets_at = unix(&r["nextResetAtMs"], true);
        w.period = Some(Duration::from_secs(604800));
    }
    snapshot(identity, vec![w], now, false)
}

pub fn parse_antigravity(
    bytes: &[u8],
    identity: &Identity,
    now: SystemTime,
) -> Result<Snapshot, FetchError> {
    let v = json(bytes)?;
    crate::antigravity::boundary::successful_code(&v)?;
    let groups = v
        .get("groups")
        .or_else(|| v.pointer("/response/groups"))
        .or_else(|| v.pointer("/summary/groups"))
        .and_then(Value::as_array)
        .ok_or(FetchError::InvalidResponse)?;
    let mut windows = Vec::new();
    for group in groups {
        let Some(buckets) = group["buckets"].as_array() else {
            continue;
        };
        for bucket in buckets {
            let Some(id) = text(&bucket["bucketId"]).or_else(|| text(&bucket["id"])) else {
                continue;
            };
            let mut w = window(&id);
            w.label = text(&bucket["displayName"]).or_else(|| text(&bucket["name"]));
            w.model = text(&group["displayName"]).or_else(|| text(&group["name"]));
            let fraction = bucket
                .get("remainingFraction")
                .or_else(|| bucket.pointer("/remaining/remainingFraction"))
                .or_else(|| {
                    if bucket["remaining"]["case"] == "remainingFraction" {
                        bucket.pointer("/remaining/value")
                    } else {
                        None
                    }
                });
            if bucket["disabled"] != true
                && let Some(n) = fraction
                    .and_then(Value::as_f64)
                    .filter(|n| n.is_finite() && (0.0..=1.0).contains(n))
            {
                w.measurement = Measurement::Percent {
                    value: NonNegative::new(n * 100.0).unwrap(),
                    direction: Direction::Remaining,
                };
            }
            w.period = match bucket["window"].as_str() {
                Some("weekly") => Some(Duration::from_secs(604800)),
                Some("five-hour") => Some(Duration::from_secs(18000)),
                _ => None,
            };
            w.resets_at = iso(&bucket["resetTime"]);
            windows.push(w);
        }
    }
    if windows
        .iter()
        .all(|w| w.measurement == Measurement::Unknown)
    {
        return Err(FetchError::TemporarilyUnavailable);
    }
    snapshot(identity, windows, now, false)
}
