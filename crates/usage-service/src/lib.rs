mod policy;
mod settings;
pub use usage_providers::credential_helper_main;

use policy::Policy;
use std::{
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::{
    sync::{mpsc, watch},
    task::{AbortHandle, JoinSet},
};
use usage_core::{
    Begin, Cache, FetchError, FetchState, Freshness, Provider, RequestToken, Snapshot,
};
use usage_providers::{
    Cancellation, CredentialRevision, PreparedProbe, ProbeFailure, ProviderClient,
};

pub const PROVIDERS: [Provider; 4] = [
    Provider::Claude,
    Provider::Codex,
    Provider::GrokBot,
    Provider::Antigravity,
];

#[derive(Clone, Copy, Debug)]
pub enum Command {
    Refresh(Option<Provider>),
    SetEnabled(Provider, bool),
    Wake,
    Shutdown,
}

#[derive(Clone, Debug)]
pub struct ProviderStatus {
    pub provider: Provider,
    pub enabled: bool,
    pub snapshot: Option<Snapshot>,
    pub freshness: Freshness,
    pub fetch_state: FetchState,
    pub next_refresh: Option<SystemTime>,
}

pub fn initial_statuses() -> Vec<ProviderStatus> {
    PROVIDERS
        .into_iter()
        .map(|provider| ProviderStatus {
            provider,
            enabled: true,
            snapshot: None,
            freshness: Freshness::Missing,
            fetch_state: FetchState::Idle,
            next_refresh: None,
        })
        .collect()
}

struct Task {
    handle: AbortHandle,
    cancellation: Cancellation,
}

struct Slot {
    policy: Policy,
    cache: Option<Cache>,
    revision: Option<CredentialRevision>,
    generation: u64,
    task: Option<Task>,
    token: Option<RequestToken>,
    error: Option<FetchError>,
    expiry_announced: bool,
}

impl Slot {
    fn set_enabled(&mut self, enabled: bool) {
        if let Some(task) = &self.task {
            task.cancellation.cancel();
        }
        self.token = None;
        self.generation = self
            .generation
            .checked_add(1)
            .expect("request generation exhausted");
        self.cache = None;
        self.revision = None;
        self.error = None;
        self.policy.set_enabled(enabled, Instant::now());
    }

    fn status(&self, index: usize, now: SystemTime, monotonic: Instant) -> ProviderStatus {
        let view = self.cache.as_ref().map(|cache| cache.view(now));
        ProviderStatus {
            provider: PROVIDERS[index],
            enabled: self.policy.enabled,
            snapshot: view.as_ref().and_then(|view| view.snapshot.cloned()),
            freshness: view
                .as_ref()
                .map_or(Freshness::Missing, |view| view.freshness),
            fetch_state: if self.policy.busy {
                FetchState::Refreshing
            } else if let Some(error) = self.error {
                FetchState::Failed(error)
            } else {
                FetchState::Idle
            },
            next_refresh: self
                .policy
                .due
                .and_then(|due| now.checked_add(due.saturating_duration_since(monotonic))),
        }
    }
}

enum Completion {
    Prepared {
        index: usize,
        generation: u64,
        result: Result<PreparedProbe, ProbeFailure>,
    },
    Fetched {
        index: usize,
        generation: u64,
        token: RequestToken,
        result: Result<Snapshot, ProbeFailure>,
    },
}

pub async fn run(
    mut commands: mpsc::Receiver<Command>,
    updates: watch::Sender<Vec<ProviderStatus>>,
) {
    let enabled = settings::load();
    let now = Instant::now();
    let mut slots: Vec<_> = enabled
        .into_iter()
        .map(|enabled| Slot {
            policy: Policy::new(enabled, now),
            cache: None,
            revision: None,
            generation: 0,
            task: None,
            token: None,
            error: None,
            expiry_announced: false,
        })
        .collect();
    let client = match ProviderClient::new() {
        Ok(client) => Arc::new(client),
        Err(error) => {
            for slot in &mut slots {
                slot.error = Some(error);
                slot.policy.due = None;
            }
            publish(&slots, &updates);
            return;
        }
    };
    let mut tasks = JoinSet::new();
    let mut jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u64;
    loop {
        let now = Instant::now();
        let wall = SystemTime::now();
        let mut busy = slots.iter().filter(|slot| slot.policy.busy).count();
        for (index, slot) in slots.iter_mut().enumerate() {
            if busy < 2 && slot.policy.start(now) {
                slot.generation = slot
                    .generation
                    .checked_add(1)
                    .expect("request generation exhausted");
                let generation = slot.generation;
                let client = client.clone();
                let cancellation = Cancellation::default();
                let stop = cancellation.clone();
                slot.task = Some(Task {
                    cancellation,
                    handle: tasks.spawn(async move {
                        Completion::Prepared {
                            index,
                            generation,
                            result: client.prepare_cancellable(PROVIDERS[index], &stop).await,
                        }
                    }),
                });
                busy += 1;
            }
            if slot
                .cache
                .as_ref()
                .is_some_and(|cache| cache.view(wall).freshness == Freshness::Stale)
            {
                slot.expiry_announced = true;
            }
        }
        publish(&slots, &updates);
        let deadline = next_deadline(&slots, now, wall);
        tokio::select! {
            command = commands.recv() => match command {
                None | Some(Command::Shutdown) => break,
                Some(Command::Refresh(provider)) => {
                    for (index, slot) in slots.iter_mut().enumerate() {
                        if provider.is_none_or(|provider| provider == PROVIDERS[index]) { slot.policy.request(Instant::now(), true); }
                    }
                }
                Some(Command::Wake) => for slot in &mut slots { slot.policy.request(Instant::now(), false); },
                Some(Command::SetEnabled(provider, enabled)) => {
                    let index = PROVIDERS.iter().position(|value| *value == provider).expect("provider registry");
                    let slot = &mut slots[index];
                    if slot.policy.enabled != enabled {
                        slot.set_enabled(enabled);
                        let enabled = std::array::from_fn(|i| slots[i].policy.enabled);
                        if settings::save(enabled).is_err() { eprintln!("preferences_write_failed"); }
                    }
                }
            },
            completion = tasks.join_next(), if !tasks.is_empty() => {
                match completion {
                    Some(Ok(completion)) => handle_completion(completion, &mut slots, &mut tasks, &client, &mut jitter),
                    Some(Err(error)) => {
                        if let Some(slot) = slots.iter_mut().find(|slot| slot.task.as_ref().is_some_and(|task| task.handle.id() == error.id())) {
                            slot.task = None;
                            if let (Some(cache), Some(token)) = (&mut slot.cache, slot.token.take()) { let _ = cache.cancel(&token); }
                            finish(slot, Some(FetchError::TemporarilyUnavailable.into()), &mut jitter);
                        }
                    }
                    None => {}
                }
            },
            () = sleep_until(deadline) => {}
        }
    }
    for slot in &slots {
        if let Some(task) = &slot.task {
            task.cancellation.cancel();
        }
    }
    while tasks.join_next().await.is_some() {}
}

fn handle_completion(
    completion: Completion,
    slots: &mut [Slot],
    tasks: &mut JoinSet<Completion>,
    client: &Arc<ProviderClient>,
    jitter: &mut u64,
) {
    let (index, generation) = match &completion {
        Completion::Prepared {
            index, generation, ..
        }
        | Completion::Fetched {
            index, generation, ..
        } => (*index, *generation),
    };
    let slot = &mut slots[index];
    if slot.generation != generation {
        slot.task = None;
        slot.policy.cancelled(Instant::now());
        return;
    }
    slot.task = None;
    slot.token = None;
    match completion {
        Completion::Prepared {
            result: Ok(probe), ..
        } => {
            if slot.revision.as_ref() != Some(probe.credential_revision())
                || slot
                    .cache
                    .as_ref()
                    .is_none_or(|cache| cache.selected() != probe.identity())
            {
                slot.cache = Some(Cache::new(probe.identity().clone()));
                slot.revision = Some(probe.credential_revision().clone());
            }
            let cache = slot.cache.as_mut().expect("prepared cache");
            let Begin::Started(token) = cache.begin() else {
                unreachable!("service starts one request per provider")
            };
            let client = client.clone();
            slot.token = Some(token.clone());
            let cancellation = Cancellation::default();
            let stop = cancellation.clone();
            slot.task = Some(Task {
                cancellation,
                handle: tasks.spawn(async move {
                    Completion::Fetched {
                        index,
                        generation,
                        token,
                        result: client.fetch_cancellable(probe, &stop).await,
                    }
                }),
            });
        }
        Completion::Prepared {
            result: Err(failure),
            ..
        } => {
            slot.cache = None;
            slot.revision = None;
            finish(slot, Some(failure), jitter);
        }
        Completion::Fetched { token, result, .. } => match result {
            Ok(snapshot) => {
                let result = slot
                    .cache
                    .as_mut()
                    .expect("fetch cache")
                    .succeed(&token, snapshot);
                if result.is_ok() {
                    slot.expiry_announced = false;
                    finish(slot, None, jitter);
                } else {
                    finish(slot, Some(FetchError::NeedsVerification.into()), jitter);
                }
            }
            Err(failure) => {
                if failure.identity_invalidated {
                    slot.cache = None;
                    slot.revision = None;
                } else if let Some(cache) = &mut slot.cache {
                    let _ = cache.fail(&token, failure.error);
                }
                finish(slot, Some(failure), jitter);
            }
        },
    }
}

fn finish(slot: &mut Slot, failure: Option<ProbeFailure>, jitter: &mut u64) {
    *jitter = jitter.wrapping_mul(6364136223846793005).wrapping_add(1);
    slot.error = failure.map(|failure| failure.error);
    slot.policy.complete(Instant::now(), failure, *jitter);
}

fn publish(slots: &[Slot], updates: &watch::Sender<Vec<ProviderStatus>>) {
    let wall = SystemTime::now();
    let monotonic = Instant::now();
    updates.send_replace(
        slots
            .iter()
            .enumerate()
            .map(|(index, slot)| slot.status(index, wall, monotonic))
            .collect(),
    );
}

fn next_deadline(slots: &[Slot], now: Instant, wall: SystemTime) -> Option<Instant> {
    let full = slots.iter().filter(|slot| slot.policy.busy).count() >= 2;
    slots
        .iter()
        .flat_map(|slot| {
            let due = slot
                .policy
                .due
                .filter(|_| slot.policy.enabled && !slot.policy.busy && !full);
            let expiry = if !slot.expiry_announced {
                slot.cache
                    .as_ref()
                    .and_then(|cache| cache.view(wall).snapshot)
                    .and_then(|snapshot| snapshot.timing.expires_at.duration_since(wall).ok())
                    .and_then(|duration| now.checked_add(duration))
            } else {
                None
            };
            [due, expiry].into_iter().flatten()
        })
        .min()
}

async fn sleep_until(deadline: Option<Instant>) {
    match deadline {
        Some(deadline) => tokio::time::sleep_until(deadline.into()).await,
        None => std::future::pending::<()>().await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn disable_and_reenable_hold_capacity_until_cleanup_is_joined() {
        let now = Instant::now();
        let mut slots: Vec<_> = (0..4)
            .map(|_| Slot {
                policy: Policy::new(true, now),
                cache: None,
                revision: None,
                generation: 1,
                task: None,
                token: None,
                error: None,
                expiry_announced: false,
            })
            .collect();
        assert!(slots[0].policy.start(now));
        assert!(slots[1].policy.start(now));
        let mut tasks = JoinSet::new();
        let cancellation = Cancellation::default();
        let stop = cancellation.clone();
        let (cleanup, cleaned) = tokio::sync::oneshot::channel();
        slots[0].task = Some(Task {
            cancellation,
            handle: tasks.spawn(async move {
                stop.cancelled().await;
                cleaned.await.unwrap();
                Completion::Prepared {
                    index: 0,
                    generation: 1,
                    result: Err(FetchError::TemporarilyUnavailable.into()),
                }
            }),
        });
        slots[0].set_enabled(false);
        slots[0].set_enabled(true);
        assert_eq!(slots.iter().filter(|slot| slot.policy.busy).count(), 2);
        assert!(next_deadline(&slots, now, SystemTime::now()).is_none());
        assert!(
            !slots[0]
                .policy
                .start(now + std::time::Duration::from_secs(60))
        );
        assert!(tasks.try_join_next().is_none());
        cleanup.send(()).unwrap();
        let completion = tasks.join_next().await.unwrap().unwrap();
        handle_completion(
            completion,
            &mut slots,
            &mut tasks,
            &Arc::new(ProviderClient::new().unwrap()),
            &mut 0,
        );
        assert!(!slots[0].policy.busy);
        assert!(slots[0].task.is_none());
        assert_eq!(slots.iter().filter(|slot| slot.policy.busy).count(), 1);
        assert!(
            slots[0]
                .policy
                .start(now + std::time::Duration::from_secs(60))
        );
    }
}
