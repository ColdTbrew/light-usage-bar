use std::{
    io::{self, Cursor, Read},
    time::{Duration, UNIX_EPOCH},
};
use usage_core::*;
use usage_display::*;

#[test]
fn projection_preserves_quota_and_omits_identity_and_raw_derivation() {
    let snapshot = Snapshot {
        identity: Identity {
            provider: Provider::Claude,
            account: AccountId::new("private-account").unwrap(),
            workspace: Some(WorkspaceId::new("private-workspace").unwrap()),
        },
        windows: vec![QuotaWindow {
            id: Some(WindowId::new("burst").unwrap()),
            label: None,
            model: None,
            measurement: Measurement::Amount {
                value: NonNegative::new(142.).unwrap(),
                limit: Some(NonNegative::new(100.).unwrap()),
                direction: Direction::Remaining,
                unit: Unit::ProviderDefined(ProviderUnit::new(" custom units ").unwrap()),
            },
            period: None,
            period_started_at: None,
            resets_at: Some(UNIX_EPOCH + Duration::from_secs(20)),
            trial_expires_at: Some(UNIX_EPOCH + Duration::from_secs(40)),
        }],
        timing: Timing {
            observed_at: None,
            fetched_at: UNIX_EPOCH,
            expires_at: UNIX_EPOCH,
        },
        source: Source {
            kind: SourceKind::LocalLog,
            provenance: Provenance::Derived(Derivation::new("private derivation inputs").unwrap()),
        },
    };
    let display = project_provider(
        Provider::Claude,
        true,
        Some(&snapshot),
        Freshness::Stale,
        FetchState::Failed(FetchError::RateLimited),
        None,
    );
    let message = HostToPanel::Replace {
        snapshot: DisplaySnapshot {
            revision: 3,
            demo: false,
            providers: vec![display],
        },
    };
    let bytes = encode(&message).unwrap();
    let wire = String::from_utf8(bytes.clone()).unwrap();
    assert!(!wire.contains("private-account"));
    assert!(!wire.contains("private-workspace"));
    assert!(!wire.contains("private derivation inputs"));
    let HostToPanel::Replace { snapshot } = decode(&bytes).unwrap() else {
        panic!()
    };
    let provider = &snapshot.providers[0];
    assert_eq!(provider.freshness, DisplayFreshness::Stale);
    assert_eq!(provider.fetch, DisplayFetch::RateLimited);
    let observation = provider.last_success.as_ref().unwrap();
    assert_eq!(observation.source.provenance, DisplayProvenance::Derived);
    let window = &observation.windows[0];
    assert_eq!(window.period_seconds, None);
    assert_eq!(window.model, None);
    assert_eq!(window.period_started_at_ms, None);
    assert_eq!(window.resets_at_ms, Some(20000));
    assert_eq!(window.trial_expires_at_ms, Some(40000));
    assert_eq!(
        window.measurement,
        DisplayMeasurement::Amount {
            value: 142.,
            limit: Some(100.),
            direction: DisplayDirection::Remaining,
            unit: " custom units ".into()
        }
    );
}
#[test]
fn missing_is_unknown_and_frames_round_trip() {
    let message = HostToPanel::Replace {
        snapshot: empty_snapshot(false),
    };
    let mut bytes = vec![];
    write_frame(&mut bytes, &message).unwrap();
    let decoded: HostToPanel = read_frame(&mut Cursor::new(bytes)).unwrap();
    assert_eq!(decoded, message);
    let display = project_provider(
        Provider::Codex,
        true,
        None,
        Freshness::Missing,
        FetchState::Failed(FetchError::NeedsAuthentication),
        None,
    );
    assert_eq!(display.last_success, None);
    assert!(
        provider_lines(&display)
            .iter()
            .any(|s| s.contains("No usage data"))
    );
}
struct HeaderOnly {
    header: Cursor<Vec<u8>>,
}
impl Read for HeaderOnly {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if self.header.position() >= 4 {
            panic!("oversized body must not be read");
        }
        self.header.read(bytes)
    }
}
#[test]
fn rejects_oversized_header_before_reading_payload_and_truncated_frames() {
    let mut reader = HeaderOnly {
        header: Cursor::new(((MAX_FRAME + 1) as u32).to_be_bytes().to_vec()),
    };
    assert_eq!(
        read_frame::<HostToPanel>(&mut reader).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    for bytes in [vec![], vec![0, 0], vec![0, 0, 0, 5, b'{']] {
        assert!(read_frame::<HostToPanel>(&mut Cursor::new(bytes)).is_err());
    }
}
#[test]
fn rejects_unknown_messages_invalid_numbers_and_unbounded_content() {
    assert!(decode::<PanelToHost>(br#"{"type":"RunCommand","command":"anything"}"#).is_err());
    assert!(decode::<PanelToHost>(br#"{"type":"Quit","unexpected":1}"#).is_err());
    let mut snapshot = demo_snapshot();
    snapshot.providers[0].last_success.as_mut().unwrap().windows[0].measurement =
        DisplayMeasurement::Percent {
            value: f64::INFINITY,
            direction: DisplayDirection::Used,
        };
    assert!(encode(&HostToPanel::Replace { snapshot }).is_err());
    let mut snapshot = demo_snapshot();
    snapshot.providers[0].last_success.as_mut().unwrap().windows[0].label = Some("x".repeat(513));
    assert!(encode(&HostToPanel::Replace { snapshot }).is_err());
    assert!(
        encode(&HostToPanel::Hello {
            protocol: PROTOCOL + 1,
            session: 1,
            anchor: ScreenAnchor {
                x: 0.,
                y: 0.,
                width: 1.,
                height: 1.
            }
        })
        .is_err()
    );
}
