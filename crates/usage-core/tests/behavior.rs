use std::time::{Duration, SystemTime, UNIX_EPOCH};
use usage_core::*;

fn time(seconds: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(seconds)
}

fn identity(account: &str) -> Identity {
    Identity {
        provider: Provider::Codex,
        account: AccountId::new(account).unwrap(),
        workspace: None,
    }
}

fn snapshot(identity: Identity) -> Snapshot {
    Snapshot {
        identity,
        windows: vec![QuotaWindow {
            id: None,
            label: None,
            measurement: Measurement::Percent {
                value: NonNegative::new(42.0).unwrap(),
                direction: Direction::Used,
            },
            model: None,
            period: None,
            period_started_at: None,
            resets_at: Some(time(15)),
            trial_expires_at: Some(time(500)),
        }],
        timing: Timing {
            observed_at: Some(time(5)),
            fetched_at: time(10),
            expires_at: time(20),
        },
        source: Source {
            kind: SourceKind::ProviderClient,
            provenance: Provenance::ProviderReported,
        },
    }
}

fn start(cache: &mut Cache) -> RequestToken {
    match cache.begin() {
        Begin::Started(token) => token,
        Begin::Joined(_) => panic!("expected new work"),
    }
}

fn seed(cache: &mut Cache) -> Snapshot {
    let success = snapshot(cache.selected().clone());
    let token = start(cache);
    cache.succeed(&token, success.clone()).unwrap();
    success
}

#[test]
fn observation_identity_is_fresh_clone_preserving_and_redacted() {
    let first = AccountId::observation();
    let second = AccountId::observation();
    assert_eq!(first, first.clone());
    assert_ne!(first, second);
    assert_ne!(first, AccountId::new("observation").unwrap());
    assert_eq!(first.binding(), AccountBinding::Observation);
    assert_eq!(
        AccountId::new("key").unwrap().binding(),
        AccountBinding::Keyed
    );
    assert_eq!(format!("{first:?}"), "AccountId([redacted])");
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
}

#[test]
fn new_observation_rejects_old_completions_and_snapshots() {
    let first = Identity {
        provider: Provider::Claude,
        account: AccountId::observation(),
        workspace: None,
    };
    let second = Identity {
        account: AccountId::observation(),
        ..first.clone()
    };
    let mut cache = Cache::new(first.clone());
    seed(&mut cache);
    let old = start(&mut cache);
    cache.select(second.clone());
    assert!(cache.view(time(10)).snapshot.is_none());
    let current = start(&mut cache);
    assert_eq!(
        cache.succeed(&old, snapshot(first.clone())),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(
        cache.succeed(&current, snapshot(first)),
        Err(CompletionError::IdentityMismatch)
    );
    cache.select(second.clone());
    cache.succeed(&current, snapshot(second.clone())).unwrap();
    assert_eq!(cache.view(time(10)).snapshot.unwrap().identity, second);
}

#[test]
fn rejects_invalid_numbers_and_preserves_zero_and_overage() {
    for value in [-1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(NonNegative::new(value), Err(InvalidNumber));
    }
    assert_eq!(NonNegative::new(0.0).unwrap().get(), 0.0);
    assert_eq!(NonNegative::new(142.5).unwrap().get(), 142.5);
}

#[test]
fn empty_keys_are_rejected_without_normalizing_accepted_keys() {
    for value in ["", " \t\n", "\u{2003}"] {
        assert_eq!(AccountId::new(value), Err(EmptyIdentifier));
        assert_eq!(WorkspaceId::new(value), Err(EmptyIdentifier));
        assert_eq!(WindowId::new(value), Err(EmptyIdentifier));
        assert_eq!(ProviderUnit::new(value), Err(EmptyIdentifier));
    }
    assert_eq!(AccountId::new(" account "), AccountId::new(" account "));
    assert_ne!(AccountId::new(" account "), AccountId::new("account"));
    assert_eq!(WorkspaceId::new(" team "), WorkspaceId::new(" team "));
    assert_ne!(WorkspaceId::new(" team "), WorkspaceId::new("team"));
    assert_eq!(WindowId::new(" window ").unwrap().as_str(), " window ");
    assert_eq!(
        ProviderUnit::new(" compute units ").unwrap().as_str(),
        " compute units "
    );
}

#[test]
fn window_keys_and_supplied_metadata_survive_cache_without_inference() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let mut success = snapshot(cache.selected().clone());
    let mut first = success.windows[0].clone();
    first.id = Some(WindowId::new("standard").unwrap());
    first.label = Some("Standard allowance".into());
    first.model = Some("synthetic-model".into());
    first.period = Some(Duration::from_secs(3600));
    first.period_started_at = Some(time(1));
    let mut second = first.clone();
    second.id = Some(WindowId::new("bonus").unwrap());
    second.label = None;
    second.period_started_at = None;
    success.windows = vec![first, second];
    let token = start(&mut cache);
    cache.succeed(&token, success.clone()).unwrap();
    let stored = cache.view(time(11)).snapshot.unwrap();
    assert_eq!(stored, &success);
    assert_eq!(stored.windows[0].id.as_ref().unwrap().as_str(), "standard");
    assert_eq!(stored.windows[1].id.as_ref().unwrap().as_str(), "bonus");
    assert_eq!(
        stored.windows[0].label.as_deref(),
        Some("Standard allowance")
    );
    assert_eq!(stored.windows[0].period_started_at, Some(time(1)));
    assert_eq!(stored.windows[1].period_started_at, None);
    assert_eq!(stored.windows[0].resets_at, Some(time(15)));
    assert_eq!(stored.windows[0].trial_expires_at, Some(time(500)));
}

#[test]
fn derived_quota_retains_explanation_units_and_common_source() {
    for explanation in ["", " \t\n"] {
        assert_eq!(Derivation::new(explanation), Err(EmptyDerivation));
    }
    let explanation =
        "  20 used of 80 provider credits; assume both values cover the same window.  ";
    for kind in [SourceKind::LocalService, SourceKind::CommandLine] {
        let mut cache = Cache::new(identity("synthetic-a"));
        let mut success = snapshot(cache.selected().clone());
        success.windows[0].measurement = Measurement::Amount {
            value: NonNegative::new(20.0).unwrap(),
            limit: Some(NonNegative::new(80.0).unwrap()),
            direction: Direction::Used,
            unit: Unit::ProviderDefined(ProviderUnit::new(" provider credits ").unwrap()),
        };
        success.source = Source {
            kind,
            provenance: Provenance::Derived(Derivation::new(explanation).unwrap()),
        };
        let token = start(&mut cache);
        cache.succeed(&token, success.clone()).unwrap();
        let stored = cache.view(time(11)).snapshot.unwrap();
        assert_eq!(stored, &success);
        let Provenance::Derived(derivation) = &stored.source.provenance else {
            panic!("expected supplied derivation");
        };
        assert_eq!(derivation.explanation(), explanation);
        let Measurement::Amount {
            unit: Unit::ProviderDefined(unit),
            ..
        } = &stored.windows[0].measurement
        else {
            panic!("expected provider unit");
        };
        assert_eq!(unit.as_str(), " provider credits ");
    }
}

#[test]
fn missing_and_zero_are_distinct_and_measurement_meanings_survive_cache() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let mut success = snapshot(cache.selected().clone());
    success.windows = vec![
        QuotaWindow {
            id: None,
            label: None,
            measurement: Measurement::Unknown,
            model: None,
            period: None,
            period_started_at: None,
            resets_at: None,
            trial_expires_at: None,
        },
        QuotaWindow {
            id: None,
            label: None,
            measurement: Measurement::Percent {
                value: NonNegative::new(0.0).unwrap(),
                direction: Direction::Remaining,
            },
            model: Some("synthetic-model".into()),
            period: Some(Duration::from_secs(3600)),
            period_started_at: None,
            resets_at: None,
            trial_expires_at: Some(time(50)),
        },
        QuotaWindow {
            id: None,
            label: None,
            measurement: Measurement::Amount {
                value: NonNegative::new(145.0).unwrap(),
                limit: Some(NonNegative::new(100.0).unwrap()),
                direction: Direction::Used,
                unit: Unit::Requests,
            },
            model: None,
            period: None,
            period_started_at: None,
            resets_at: None,
            trial_expires_at: None,
        },
    ];
    success.source = Source {
        kind: SourceKind::LocalLog,
        provenance: Provenance::ProviderReported,
    };
    let token = start(&mut cache);
    cache.succeed(&token, success.clone()).unwrap();
    let stored = cache.view(time(11)).snapshot.unwrap();
    assert_eq!(stored, &success);
    assert_ne!(stored.windows[0].measurement, stored.windows[1].measurement);
    assert_eq!(stored.windows[0].id, None);
    assert_eq!(stored.windows[0].model, None);
    assert_eq!(stored.windows[0].label, None);
    assert_eq!(stored.windows[0].period_started_at, None);
    assert_eq!(stored.windows[0].period, None);
    assert_eq!(stored.windows[0].resets_at, None);
}

#[test]
fn single_flight_join_can_complete_once() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let token = start(&mut cache);
    let joined = match cache.begin() {
        Begin::Joined(token) => token,
        Begin::Started(_) => panic!("duplicate work"),
    };
    assert_eq!(cache.view(time(10)).fetch_state, FetchState::Refreshing);
    cache
        .succeed(&joined, snapshot(identity("synthetic-a")))
        .unwrap();
    assert_eq!(
        cache.fail(&token, FetchError::InvalidResponse),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(
        cache.succeed(&joined, snapshot(identity("synthetic-a"))),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(cache.cancel(&joined), Err(CompletionError::ObsoleteRequest));
    assert_eq!(cache.view(time(10)).fetch_state, FetchState::Idle);
    assert_eq!(cache.view(time(10)).freshness, Freshness::Fresh);
}

#[test]
fn failure_without_success_is_explicitly_missing() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let token = start(&mut cache);
    cache.fail(&token, FetchError::NeedsAuthentication).unwrap();
    let view = cache.view(time(10));
    assert_eq!(view.snapshot, None);
    assert_eq!(view.freshness, Freshness::Missing);
    assert_eq!(
        view.fetch_state,
        FetchState::Failed(FetchError::NeedsAuthentication)
    );
}

#[test]
fn errors_and_freshness_are_independent_and_reads_preserve_success() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let success = seed(&mut cache);
    let token = start(&mut cache);
    cache.fail(&token, FetchError::RateLimited).unwrap();
    for (now, expected) in [
        (11, Freshness::Fresh),
        (20, Freshness::Stale),
        (900, Freshness::Stale),
    ] {
        let view = cache.view(time(now));
        assert_eq!(view.snapshot, Some(&success));
        assert_eq!(view.freshness, expected);
        assert_eq!(
            view.fetch_state,
            FetchState::Failed(FetchError::RateLimited)
        );
    }
    let token = start(&mut cache);
    cache.cancel(&token).unwrap();
    assert_eq!(cache.view(time(900)).snapshot, Some(&success));
    assert_eq!(cache.view(time(900)).fetch_state, FetchState::Idle);
}

#[test]
fn old_provider_quota_retained_in_local_log_remains_stale_on_ingestion() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let mut success = snapshot(cache.selected().clone());
    success.timing = Timing {
        observed_at: Some(time(1)),
        fetched_at: time(100),
        expires_at: time(2),
    };
    success.source = Source {
        kind: SourceKind::LocalLog,
        provenance: Provenance::ProviderReported,
    };
    let token = start(&mut cache);
    cache.succeed(&token, success.clone()).unwrap();
    let view = cache.view(time(100));
    assert_eq!(view.snapshot, Some(&success));
    assert_eq!(view.freshness, Freshness::Stale);
    assert_eq!(view.snapshot.unwrap().timing.observed_at, Some(time(1)));
    assert_eq!(view.snapshot.unwrap().timing.fetched_at, time(100));
    assert_eq!(view.snapshot.unwrap().timing.expires_at, time(2));
    assert_eq!(view.snapshot.unwrap().source.kind, SourceKind::LocalLog);
    assert_eq!(
        view.snapshot.unwrap().source.provenance,
        Provenance::ProviderReported
    );
}

#[test]
fn quota_reset_does_not_refresh_values_or_expiry() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let success = seed(&mut cache);
    assert_eq!(cache.view(time(16)).freshness, Freshness::Fresh);
    assert_eq!(cache.view(time(20)).freshness, Freshness::Stale);
    assert_eq!(cache.view(time(1000)).snapshot, Some(&success));
}

#[test]
fn account_workspace_and_provider_changes_clear_visible_success() {
    let a = identity("synthetic-a");
    let mut workspace = a.clone();
    workspace.workspace = Some(WorkspaceId::new("synthetic-workspace").unwrap());
    let mut provider = a.clone();
    provider.provider = Provider::Claude;
    for selected in [identity("synthetic-b"), workspace, provider] {
        let mut cache = Cache::new(a.clone());
        seed(&mut cache);
        let old = start(&mut cache);
        cache.select(selected.clone());
        assert_eq!(cache.selected(), &selected);
        assert_eq!(cache.view(time(10)).snapshot, None);
        assert_eq!(cache.view(time(10)).fetch_state, FetchState::Idle);
        assert_eq!(
            cache.succeed(&old, snapshot(a.clone())),
            Err(CompletionError::ObsoleteRequest)
        );
    }
}

#[test]
fn account_round_trip_old_failure_and_cancel_cannot_replace_new_request() {
    let a = identity("synthetic-a");
    let mut cache = Cache::new(a.clone());
    let old = start(&mut cache);
    cache.select(identity("synthetic-b"));
    cache.select(a.clone());
    let current = start(&mut cache);
    assert_eq!(
        cache.fail(&old, FetchError::PermissionDenied),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(cache.cancel(&old), Err(CompletionError::ObsoleteRequest));
    assert_eq!(
        cache.succeed(&old, snapshot(a.clone())),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(cache.view(time(10)).fetch_state, FetchState::Refreshing);
    cache.succeed(&current, snapshot(a)).unwrap();
    let cancelled = start(&mut cache);
    cache.cancel(&cancelled).unwrap();
    let next = start(&mut cache);
    assert_eq!(
        cache.fail(&cancelled, FetchError::InvalidResponse),
        Err(CompletionError::ObsoleteRequest)
    );
    cache.cancel(&next).unwrap();
}

#[test]
fn selecting_same_identity_preserves_success_and_pending_work() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let success = seed(&mut cache);
    let token = start(&mut cache);
    cache.select(identity("synthetic-a"));
    assert_eq!(cache.view(time(10)).snapshot, Some(&success));
    cache
        .fail(&token, FetchError::TemporarilyUnavailable)
        .unwrap();
}

#[test]
fn mismatched_snapshot_does_not_consume_valid_work() {
    let mut cache = Cache::new(identity("synthetic-a"));
    let success = seed(&mut cache);
    let token = start(&mut cache);
    assert_eq!(
        cache.succeed(&token, snapshot(identity("synthetic-b"))),
        Err(CompletionError::IdentityMismatch)
    );
    assert_eq!(cache.view(time(10)).snapshot, Some(&success));
    assert_eq!(cache.view(time(10)).fetch_state, FetchState::Refreshing);
    cache
        .succeed(&token, snapshot(identity("synthetic-a")))
        .unwrap();
}

#[test]
fn tokens_from_independent_caches_cannot_complete_or_cancel_work() {
    let a = identity("synthetic-a");
    let mut first = Cache::new(a.clone());
    let mut second = Cache::new(a.clone());
    let first_token = start(&mut first);
    let second_token = start(&mut second);
    assert_eq!(
        second.succeed(&first_token, snapshot(a.clone())),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(
        second.fail(&first_token, FetchError::Unsupported),
        Err(CompletionError::ObsoleteRequest)
    );
    assert_eq!(
        second.cancel(&first_token),
        Err(CompletionError::ObsoleteRequest)
    );
    second.succeed(&second_token, snapshot(a.clone())).unwrap();
    first.succeed(&first_token, snapshot(a)).unwrap();
}

#[test]
fn identity_diagnostics_redact_original_account_and_workspace() {
    let mut selected = identity("private-account-value");
    selected.workspace = Some(WorkspaceId::new("private-workspace-value").unwrap());
    let mut cache = Cache::new(selected);
    seed(&mut cache);
    let diagnostics = format!("{cache:?} {:?}", cache.view(time(10)));
    assert!(!diagnostics.contains("private-account-value"));
    assert!(!diagnostics.contains("private-workspace-value"));
}
