use serde::{Deserialize, Serialize};
use std::io::{self, Read, Write};
use std::time::{SystemTime, UNIX_EPOCH};
use usage_core as core;

pub const PROTOCOL: u16 = 2;
pub const MAX_FRAME: usize = 256 * 1024;
const MAX_TEXT: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayProvider {
    Claude,
    Codex,
    GrokBot,
    Antigravity,
}
impl DisplayProvider {
    pub const ALL: [Self; 4] = [Self::Claude, Self::Codex, Self::GrokBot, Self::Antigravity];
    pub fn name(self) -> &'static str {
        match self {
            Self::Claude => "Claude",
            Self::Codex => "Codex",
            Self::GrokBot => "Grok Bot",
            Self::Antigravity => "Antigravity",
        }
    }
}
impl From<core::Provider> for DisplayProvider {
    fn from(value: core::Provider) -> Self {
        match value {
            core::Provider::Claude => Self::Claude,
            core::Provider::Codex => Self::Codex,
            core::Provider::GrokBot => Self::GrokBot,
            core::Provider::Antigravity => Self::Antigravity,
        }
    }
}
impl From<DisplayProvider> for core::Provider {
    fn from(value: DisplayProvider) -> Self {
        match value {
            DisplayProvider::Claude => Self::Claude,
            DisplayProvider::Codex => Self::Codex,
            DisplayProvider::GrokBot => Self::GrokBot,
            DisplayProvider::Antigravity => Self::Antigravity,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySnapshot {
    pub revision: u64,
    pub demo: bool,
    pub providers: Vec<ProviderDisplay>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderDisplay {
    pub provider: DisplayProvider,
    pub enabled: bool,
    pub last_success: Option<DisplayObservation>,
    pub freshness: DisplayFreshness,
    pub fetch: DisplayFetch,
    pub next_refresh_ms: Option<i64>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayFreshness {
    Missing,
    Fresh,
    Stale,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayFetch {
    Idle,
    Refreshing,
    NeedsAuthentication,
    PermissionDenied,
    RateLimited,
    TemporarilyUnavailable,
    Unsupported,
    NeedsVerification,
    InvalidResponse,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayObservation {
    pub account_binding: DisplayAccountBinding,
    pub windows: Vec<DisplayWindow>,
    pub observed_at_ms: Option<i64>,
    pub fetched_at_ms: i64,
    pub expires_at_ms: i64,
    pub source: DisplaySource,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayAccountBinding {
    Keyed,
    Observation,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplayWindow {
    pub id: Option<String>,
    pub label: Option<String>,
    pub model: Option<String>,
    pub measurement: DisplayMeasurement,
    pub period_seconds: Option<u64>,
    pub period_started_at_ms: Option<i64>,
    pub resets_at_ms: Option<i64>,
    pub trial_expires_at_ms: Option<i64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub enum DisplayMeasurement {
    Unknown {},
    Percent {
        value: f64,
        direction: DisplayDirection,
    },
    Amount {
        value: f64,
        limit: Option<f64>,
        direction: DisplayDirection,
        unit: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayDirection {
    Used,
    Remaining,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DisplaySource {
    pub kind: DisplaySourceKind,
    pub provenance: DisplayProvenance,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplaySourceKind {
    DocumentedApi,
    ProviderClient,
    LocalLog,
    LocalService,
    CommandLine,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisplayProvenance {
    ProviderReported,
    LocalObserved,
    Derived,
}

pub fn timestamp(value: SystemTime) -> i64 {
    match value.duration_since(UNIX_EPOCH) {
        Ok(d) => i64::try_from(d.as_millis()).unwrap_or(i64::MAX),
        Err(e) => -i64::try_from(e.duration().as_millis()).unwrap_or(i64::MAX),
    }
}
fn direction(value: core::Direction) -> DisplayDirection {
    match value {
        core::Direction::Used => DisplayDirection::Used,
        core::Direction::Remaining => DisplayDirection::Remaining,
    }
}
pub fn project_provider(
    provider: core::Provider,
    enabled: bool,
    snapshot: Option<&core::Snapshot>,
    freshness: core::Freshness,
    fetch: core::FetchState,
    next_refresh: Option<SystemTime>,
) -> ProviderDisplay {
    ProviderDisplay {
        provider: provider.into(),
        enabled,
        last_success: snapshot.map(|s| DisplayObservation {
            account_binding: match s.identity.account.binding() {
                core::AccountBinding::Keyed => DisplayAccountBinding::Keyed,
                core::AccountBinding::Observation => DisplayAccountBinding::Observation,
            },
            windows: s
                .windows
                .iter()
                .map(|w| DisplayWindow {
                    id: w.id.as_ref().map(|v| v.as_str().to_owned()),
                    label: w.label.clone(),
                    model: w.model.clone(),
                    measurement: match &w.measurement {
                        core::Measurement::Unknown => DisplayMeasurement::Unknown {},
                        core::Measurement::Percent {
                            value,
                            direction: d,
                        } => DisplayMeasurement::Percent {
                            value: value.get(),
                            direction: direction(*d),
                        },
                        core::Measurement::Amount {
                            value,
                            limit,
                            direction: d,
                            unit,
                        } => DisplayMeasurement::Amount {
                            value: value.get(),
                            limit: limit.map(|v| v.get()),
                            direction: direction(*d),
                            unit: match unit {
                                core::Unit::Tokens => "tokens".into(),
                                core::Unit::Requests => "requests".into(),
                                core::Unit::Credits => "credits".into(),
                                core::Unit::ProviderDefined(v) => v.as_str().into(),
                            },
                        },
                    },
                    period_seconds: w.period.map(|d| d.as_secs()),
                    period_started_at_ms: w.period_started_at.map(timestamp),
                    resets_at_ms: w.resets_at.map(timestamp),
                    trial_expires_at_ms: w.trial_expires_at.map(timestamp),
                })
                .collect(),
            observed_at_ms: s.timing.observed_at.map(timestamp),
            fetched_at_ms: timestamp(s.timing.fetched_at),
            expires_at_ms: timestamp(s.timing.expires_at),
            source: DisplaySource {
                kind: match s.source.kind {
                    core::SourceKind::DocumentedApi => DisplaySourceKind::DocumentedApi,
                    core::SourceKind::ProviderClient => DisplaySourceKind::ProviderClient,
                    core::SourceKind::LocalLog => DisplaySourceKind::LocalLog,
                    core::SourceKind::LocalService => DisplaySourceKind::LocalService,
                    core::SourceKind::CommandLine => DisplaySourceKind::CommandLine,
                },
                provenance: match s.source.provenance {
                    core::Provenance::ProviderReported => DisplayProvenance::ProviderReported,
                    core::Provenance::LocalObserved => DisplayProvenance::LocalObserved,
                    core::Provenance::Derived(_) => DisplayProvenance::Derived,
                },
            },
        }),
        freshness: match freshness {
            core::Freshness::Missing => DisplayFreshness::Missing,
            core::Freshness::Fresh => DisplayFreshness::Fresh,
            core::Freshness::Stale => DisplayFreshness::Stale,
        },
        fetch: match fetch {
            core::FetchState::Idle => DisplayFetch::Idle,
            core::FetchState::Refreshing => DisplayFetch::Refreshing,
            core::FetchState::Failed(e) => match e {
                core::FetchError::NeedsAuthentication => DisplayFetch::NeedsAuthentication,
                core::FetchError::PermissionDenied => DisplayFetch::PermissionDenied,
                core::FetchError::RateLimited => DisplayFetch::RateLimited,
                core::FetchError::TemporarilyUnavailable => DisplayFetch::TemporarilyUnavailable,
                core::FetchError::Unsupported => DisplayFetch::Unsupported,
                core::FetchError::NeedsVerification => DisplayFetch::NeedsVerification,
                core::FetchError::InvalidResponse => DisplayFetch::InvalidResponse,
            },
        },
        next_refresh_ms: next_refresh.map(timestamp),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScreenAnchor {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum HostToPanel {
    Hello {
        protocol: u16,
        session: u64,
        anchor: ScreenAnchor,
    },
    Replace {
        snapshot: DisplaySnapshot,
    },
    Close {},
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum PanelToHost {
    Ready {
        protocol: u16,
        session: u64,
    },
    Refresh {
        provider: Option<DisplayProvider>,
    },
    SetEnabled {
        provider: DisplayProvider,
        enabled: bool,
    },
    AuthGuidance {
        provider: DisplayProvider,
    },
    Quit {},
    Closed {},
}

pub trait Wire: Serialize + for<'de> Deserialize<'de> {
    fn validate(&self) -> io::Result<()>;
}
fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "invalid display protocol")
}
fn text(s: &str) -> bool {
    s.len() <= MAX_TEXT && !s.chars().any(|c| c.is_control())
}
impl Wire for HostToPanel {
    fn validate(&self) -> io::Result<()> {
        let valid = match self {
            Self::Hello {
                protocol,
                session,
                anchor,
            } => {
                *protocol == PROTOCOL
                    && *session != 0
                    && [anchor.x, anchor.y, anchor.width, anchor.height]
                        .iter()
                        .all(|v| v.is_finite())
                    && anchor.width > 0.0
                    && anchor.height > 0.0
            }
            Self::Close {} => true,
            Self::Replace { snapshot } => snapshot.validate(),
        };
        if valid { Ok(()) } else { Err(invalid()) }
    }
}
impl DisplaySnapshot {
    pub fn validate(&self) -> bool {
        self.providers.len() <= 4
            && self.providers.iter().enumerate().all(|(i, p)| {
                !self.providers[..i]
                    .iter()
                    .any(|other| other.provider == p.provider)
                    && p.last_success.as_ref().is_none_or(|s| {
                        s.windows.len() <= 32
                            && s.windows.iter().all(|w| {
                                [&w.id, &w.label, &w.model]
                                    .iter()
                                    .all(|v| v.as_ref().is_none_or(|s| text(s)))
                                    && match &w.measurement {
                                        DisplayMeasurement::Unknown {} => true,
                                        DisplayMeasurement::Percent { value, .. } => {
                                            value.is_finite() && *value >= 0.0
                                        }
                                        DisplayMeasurement::Amount {
                                            value, limit, unit, ..
                                        } => {
                                            value.is_finite()
                                                && *value >= 0.0
                                                && limit.is_none_or(|v| v.is_finite() && v >= 0.0)
                                                && text(unit)
                                                && !unit.trim().is_empty()
                                        }
                                    }
                            })
                    })
            })
    }
}
impl Wire for PanelToHost {
    fn validate(&self) -> io::Result<()> {
        if matches!(self, Self::Ready { protocol, session } if *protocol != PROTOCOL || *session == 0)
        {
            Err(invalid())
        } else {
            Ok(())
        }
    }
}
pub fn encode<T: Wire>(value: &T) -> io::Result<Vec<u8>> {
    value.validate()?;
    let bytes = serde_json::to_vec(value).map_err(|_| invalid())?;
    if bytes.len() > MAX_FRAME {
        return Err(invalid());
    }
    Ok(bytes)
}
pub fn decode<T: Wire>(bytes: &[u8]) -> io::Result<T> {
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err(invalid());
    }
    let value: T = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    value.validate()?;
    Ok(value)
}
pub fn write_frame<T: Wire>(writer: &mut impl Write, value: &T) -> io::Result<()> {
    let bytes = encode(value)?;
    writer.write_all(&(bytes.len() as u32).to_be_bytes())?;
    writer.write_all(&bytes)?;
    writer.flush()
}
pub fn read_frame<T: Wire>(reader: &mut impl Read) -> io::Result<T> {
    let mut header = [0; 4];
    reader.read_exact(&mut header)?;
    let len = u32::from_be_bytes(header) as usize;
    if len == 0 || len > MAX_FRAME {
        return Err(invalid());
    }
    let mut bytes = vec![0; len];
    reader.read_exact(&mut bytes)?;
    decode(&bytes)
}

pub fn empty_snapshot(demo: bool) -> DisplaySnapshot {
    DisplaySnapshot {
        revision: 1,
        demo,
        providers: DisplayProvider::ALL
            .into_iter()
            .map(|provider| ProviderDisplay {
                provider,
                enabled: true,
                last_success: None,
                freshness: DisplayFreshness::Missing,
                fetch: DisplayFetch::NeedsVerification,
                next_refresh_ms: None,
            })
            .collect(),
    }
}
pub fn demo_snapshot() -> DisplaySnapshot {
    let mut snapshot = empty_snapshot(true);
    let synthetic_time = 1_767_225_600_000;
    let percent_window =
        |id: &str, label: &str, value, direction, period_seconds, reset_offset| DisplayWindow {
            id: Some(id.into()),
            label: Some(label.into()),
            model: None,
            measurement: DisplayMeasurement::Percent { value, direction },
            period_seconds,
            period_started_at_ms: None,
            resets_at_ms: Some(synthetic_time + reset_offset),
            trial_expires_at_ms: None,
        };
    let mut model_window = percent_window(
        "synthetic-model-a",
        "Synthetic model A pool",
        73.0,
        DisplayDirection::Remaining,
        None,
        7_200_000,
    );
    model_window.model = Some("synthetic-model-a".into());
    let mut other_model_window = percent_window(
        "synthetic-model-b",
        "Synthetic model B pool",
        28.0,
        DisplayDirection::Remaining,
        None,
        14_400_000,
    );
    other_model_window.model = Some("synthetic-model-b".into());
    let windows = [
        vec![
            percent_window(
                "five_hour",
                "Synthetic five-hour window",
                42.0,
                DisplayDirection::Used,
                Some(18_000),
                10_800_000,
            ),
            percent_window(
                "seven_day",
                "Synthetic seven-day window",
                68.5,
                DisplayDirection::Used,
                Some(604_800),
                345_600_000,
            ),
        ],
        vec![
            percent_window(
                "synthetic-short",
                "Synthetic short window",
                24.0,
                DisplayDirection::Used,
                Some(18_000),
                7_200_000,
            ),
            percent_window(
                "synthetic-long",
                "Synthetic long window",
                57.0,
                DisplayDirection::Used,
                Some(604_800),
                432_000_000,
            ),
        ],
        vec![percent_window(
            "synthetic-usage",
            "Synthetic usage window",
            36.5,
            DisplayDirection::Used,
            None,
            86_400_000,
        )],
        vec![model_window, other_model_window],
    ];
    for (provider, windows) in snapshot.providers.iter_mut().zip(windows) {
        provider.last_success = Some(DisplayObservation {
            account_binding: DisplayAccountBinding::Keyed,
            windows,
            observed_at_ms: Some(synthetic_time),
            fetched_at_ms: synthetic_time,
            expires_at_ms: synthetic_time + 60_000,
            source: DisplaySource {
                kind: DisplaySourceKind::ProviderClient,
                provenance: DisplayProvenance::Derived,
            },
        });
        provider.freshness = DisplayFreshness::Stale;
        provider.fetch = DisplayFetch::Idle;
    }
    snapshot
}
fn date(value: i64) -> String {
    chrono::DateTime::from_timestamp_millis(value)
        .map(|v| v.format("%Y-%m-%d %H:%M UTC").to_string())
        .unwrap_or_else(|| "Unknown".into())
}

pub fn provider_lines(provider: &ProviderDisplay) -> Vec<String> {
    let mut lines = vec![format!(
        "{} · {} · {:?} · {:?}",
        provider.provider.name(),
        if provider.enabled {
            "Enabled"
        } else {
            "Disabled"
        },
        provider.freshness,
        provider.fetch
    )];
    if let Some(observation) = &provider.last_success {
        if observation.account_binding == DisplayAccountBinding::Observation {
            lines.push("Account not identified for this reading".into());
        }
        if observation.windows.is_empty() {
            lines.push("No quota windows reported".into());
        }
        for window in &observation.windows {
            let measurement = match &window.measurement {
                DisplayMeasurement::Unknown {} => "Unknown quota".into(),
                DisplayMeasurement::Percent { value, direction } => {
                    format!("{value}% {direction:?}")
                }
                DisplayMeasurement::Amount {
                    value,
                    limit,
                    direction,
                    unit,
                } => match limit {
                    Some(limit) => format!("{value} / {limit} {unit} {direction:?}"),
                    None => format!("{value} {unit} {direction:?} (limit unknown)"),
                },
            };
            lines.push(format!(
                "{}: {measurement}",
                window
                    .label
                    .as_deref()
                    .or(window.id.as_deref())
                    .unwrap_or("Quota window")
            ));
            if let Some(period) = window.period_seconds {
                lines.push(format!("Window duration: {period} seconds"));
            }
            if let Some(started) = window.period_started_at_ms {
                lines.push(format!("Window started: {}", date(started)));
            }
            if let Some(model) = &window.model {
                lines.push(format!("Model: {model}"));
            }
            lines.push(format!(
                "Reset: {}",
                window
                    .resets_at_ms
                    .map(date)
                    .unwrap_or_else(|| "Unknown".into())
            ));
            if let Some(expiry) = window.trial_expires_at_ms {
                lines.push(format!("Trial expires: {}", date(expiry)));
            }
        }
        lines.push(format!("Retrieved: {}", date(observation.fetched_at_ms)));
        if let Some(observed) = observation.observed_at_ms {
            lines.push(format!("Observed: {}", date(observed)));
        }
        lines.push(format!(
            "Source: {:?}, {:?}",
            observation.source.kind, observation.source.provenance
        ));
    } else {
        lines.push("No usage data. Missing values are unknown.".into());
    }
    lines
}
