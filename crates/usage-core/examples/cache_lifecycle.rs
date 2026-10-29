use std::time::{Duration, UNIX_EPOCH};
use usage_core::*;

fn main() {
    let identity = Identity {
        provider: Provider::Codex,
        account: AccountId::new("synthetic-a").unwrap(),
        workspace: None,
    };
    let mut cache = Cache::new(identity.clone());
    let Begin::Started(token) = cache.begin() else {
        unreachable!()
    };
    let now = UNIX_EPOCH + Duration::from_secs(100);
    cache
        .succeed(
            &token,
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
                    resets_at: None,
                    trial_expires_at: None,
                }],
                timing: Timing {
                    observed_at: None,
                    fetched_at: now,
                    expires_at: now + Duration::from_secs(600),
                },
                source: Source {
                    kind: SourceKind::ProviderClient,
                    provenance: Provenance::ProviderReported,
                },
            },
        )
        .unwrap();
    let Begin::Started(token) = cache.begin() else {
        unreachable!()
    };
    cache
        .fail(&token, FetchError::TemporarilyUnavailable)
        .unwrap();
    let view = cache.view(now);
    assert_eq!(
        view.fetch_state,
        FetchState::Failed(FetchError::TemporarilyUnavailable)
    );
    assert_eq!(
        view.snapshot.unwrap().windows[0].measurement,
        Measurement::Percent {
            value: NonNegative::new(42.0).unwrap(),
            direction: Direction::Used,
        }
    );
    println!(
        "Synthetic usage after failure: 42% used ({:?})",
        view.freshness
    );
    cache.select(Identity {
        provider: Provider::Codex,
        account: AccountId::new("synthetic-b").unwrap(),
        workspace: None,
    });
    assert!(cache.view(now).snapshot.is_none());
    println!("Synthetic usage after account switch: missing");
}
