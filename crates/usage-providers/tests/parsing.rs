use std::time::{Duration, UNIX_EPOCH};
use usage_core::*;
use usage_providers::*;
fn id(provider: Provider) -> Identity {
    Identity {
        provider,
        account: AccountId::new("synthetic-user").unwrap(),
        workspace: if provider == Provider::Codex {
            Some(WorkspaceId::new("synthetic-workspace").unwrap())
        } else {
            None
        },
    }
}
#[test]
fn missing_null_and_invalid_numbers_are_never_zero() {
    let identity = id(Provider::Claude);
    assert_eq!(
        parse_claude(b"{}", &identity, UNIX_EPOCH),
        Err(FetchError::InvalidResponse)
    );
    let s = parse_claude(br#"{"five_hour":{"utilization":null},"seven_day":{"utilization":-3},"seven_day_sonnet":null}"#, &identity, UNIX_EPOCH).unwrap();
    assert_eq!(s.windows.len(), 2);
    assert!(
        s.windows
            .iter()
            .all(|w| w.measurement == Measurement::Unknown)
    );
    assert!(
        parse_claude(
            br#"{"five_hour":{"utilization":NaN}}"#,
            &identity,
            UNIX_EPOCH
        )
        .is_err()
    );
}
#[test]
fn codex_primary_can_be_weekly_and_extra_failures_are_isolated() {
    let s = parse_codex(br#"{"user_id":"synthetic-user","account_id":"synthetic-workspace","rate_limit":{"primary_window":{"used_percent":27,"limit_window_seconds":604800,"reset_at":1000},"secondary_window":null},"additional_rate_limits":[{}, {"limit_name":"Synthetic model","metered_feature":"synthetic-model","rate_limit":{"primary_window":{"used_percent":5,"limit_window_seconds":18000}}}]}"#, &id(Provider::Codex), UNIX_EPOCH).unwrap();
    assert_eq!(s.windows.len(), 2);
    assert_eq!(s.windows[0].period, Some(Duration::from_secs(604800)));
    assert_eq!(
        s.windows[0].resets_at,
        Some(UNIX_EPOCH + Duration::from_secs(1000))
    );
    assert_eq!(s.windows[1].model.as_deref(), Some("synthetic-model"));
    assert_eq!(
        s.windows[1].id.as_ref().unwrap().as_str(),
        "additional.synthetic-model.primary_window"
    );
}
#[test]
fn codex_account_mismatch_is_rejected() {
    assert_eq!(
        parse_codex(
            br#"{"account_id":"different","rate_limit":{"primary_window":{"used_percent":10}}}"#,
            &id(Provider::Codex),
            UNIX_EPOCH
        ),
        Err(FetchError::NeedsVerification)
    );
}
#[test]
fn claude_model_limits_remain_separate() {
    let s = parse_claude(br#"{"five_hour":{"utilization":18.5},"limits":[{"kind":"weekly_scoped","group":"weekly","percent":41,"scope":{"model":{"id":"synthetic","display_name":"Synthetic"}}}]}"#, &id(Provider::Claude), UNIX_EPOCH).unwrap();
    assert_eq!(s.windows.len(), 2);
    assert_eq!(s.windows[1].period, Some(Duration::from_secs(604800)));
    assert_eq!(s.windows[1].model.as_deref(), Some("synthetic"));
}
fn grok(percent: &str, trial: bool) -> Vec<u8> {
    format!(r#"{{"jsonrpc":"2.0","id":1,"result":{{"usagePercent":{percent},"currentPeriodStartMs":1000,"nextResetAtMs":2000,"hasAvailableUsage":true,"hasNonZeroIncludedLimit":true,"includedLimitZero":false,"trial":{trial},"isTeamSeat":true,"onDemandEligible":true,"onDemandEnabled":true,"manageUrl":"https://example.invalid/usage"}}}}"#).into_bytes()
}
#[test]
fn grok_preserves_overage_and_trial_expiry_in_milliseconds() {
    let s = parse_grok(&grok("137.5", true), &id(Provider::GrokBot), UNIX_EPOCH).unwrap();
    let w = &s.windows[0];
    assert_eq!(
        w.measurement,
        Measurement::Percent {
            value: NonNegative::new(137.5).unwrap(),
            direction: Direction::Used
        }
    );
    assert_eq!(
        w.period_started_at,
        Some(UNIX_EPOCH + Duration::from_secs(1))
    );
    assert_eq!(
        w.trial_expires_at,
        Some(UNIX_EPOCH + Duration::from_secs(2))
    );
    assert_eq!(w.resets_at, None);
    assert_eq!(w.period, None);
    let unmetered = parse_grok(&grok("null", false), &id(Provider::GrokBot), UNIX_EPOCH).unwrap();
    assert_eq!(unmetered.windows[0].measurement, Measurement::Unknown);
}
#[test]
fn antigravity_unknown_is_not_a_current_success() {
    assert_eq!(parse_antigravity(br#"{"groups":[{"buckets":[{"bucketId":"disabled","disabled":true,"remainingFraction":1}]}]}"#, &id(Provider::Antigravity), UNIX_EPOCH), Err(FetchError::TemporarilyUnavailable));
    let s = parse_antigravity(br#"{"summary":{"groups":[{"displayName":"Synthetic pool","buckets":[{"bucketId":"weekly","remaining":{"case":"remainingFraction","value":0.65},"window":"weekly"},{"bucketId":"unknown","remainingFraction":3}]}]}}"#, &id(Provider::Antigravity), UNIX_EPOCH).unwrap();
    assert_eq!(
        s.windows[0].measurement,
        Measurement::Percent {
            value: NonNegative::new(65.0).unwrap(),
            direction: Direction::Remaining
        }
    );
    assert_eq!(s.windows[1].measurement, Measurement::Unknown);
    assert_eq!(s.windows[1].period, None);
}
#[test]
fn retry_after_accepts_seconds_and_http_dates() {
    assert_eq!(
        retry_after("120", UNIX_EPOCH),
        Some(Duration::from_secs(120))
    );
    assert_eq!(
        retry_after("Thu, 01 Jan 1970 00:01:00 GMT", UNIX_EPOCH),
        Some(Duration::from_secs(60))
    );
    assert_eq!(retry_after("invalid", UNIX_EPOCH), None);
}
