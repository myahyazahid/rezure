//! Restarts a service that died on its own, instead of leaving it down
//! until someone notices.
//!
//! Without this, a crashed `php-cgi` meant every request after it was a 502
//! until the user restarted PHP by hand — and nothing even noticed the exit
//! until the UI next asked for status, so a window left in the background
//! could sit on a dead PHP for hours. The watchdog here checks every
//! [`TICK`] regardless of what the UI is doing.
//!
//! Only services that opt in through [`super::Service::restarts_on_crash`] are
//! touched, and a stop by the user always wins: [`super::Service::crashed`] is
//! cleared by any stop, so a service stopped on purpose is never brought
//! back. A service that keeps dying is given up on after
//! [`BACKOFF`]'s worth of attempts inside [`RESTART_WINDOW`] rather than
//! restarted forever, and stays given up on until the user starts or
//! stops it themselves.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager};

use super::{ServiceHandle, ServiceManager};

/// Emitted whenever the supervisor changes a service's state behind the
/// UI's back — a crash noticed, a restart, a give-up — so the frontend can
/// refetch instead of showing a stale "Running".
pub const CHANGED_EVENT: &str = "service://changed";

/// How often services are checked.
const TICK: Duration = Duration::from_secs(2);

/// Delay before each successive restart inside [`RESTART_WINDOW`]. Once
/// every entry has been used up, the service is given up on.
const BACKOFF: [Duration; 5] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(5),
    Duration::from_secs(10),
    Duration::from_secs(30),
];

/// How far back restarts count against [`BACKOFF`]. A service that ran
/// fine for this long starts again from the shortest delay.
const RESTART_WINDOW: Duration = Duration::from_secs(5 * 60);

/// What to do about a crash that was just noticed.
#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    RestartAfter(Duration),
    GiveUp,
}

/// Restart budget per service — pure and clock-injected so the backoff
/// rule is testable without waiting on real time.
#[derive(Default)]
pub struct RestartPolicy {
    recent: HashMap<String, Vec<Instant>>,
}

impl RestartPolicy {
    /// Records a crash of `id` at `now` and decides whether (and after how
    /// long) it may be restarted.
    pub fn on_crash(&mut self, id: &str, now: Instant) -> Decision {
        let recent = self.recent.entry(id.to_string()).or_default();
        recent.retain(|at| now.duration_since(*at) < RESTART_WINDOW);
        match BACKOFF.get(recent.len()) {
            Some(delay) => {
                recent.push(now);
                Decision::RestartAfter(*delay)
            }
            None => Decision::GiveUp,
        }
    }

    /// Drops `id`'s history — once the user has stepped in after a give-up,
    /// the next crash starts from a clean budget.
    pub fn forget(&mut self, id: &str) {
        self.recent.remove(id);
    }
}

/// Something the supervisor did, reported so the caller can log, notify
/// and tell the frontend.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// A crash was noticed and a restart scheduled.
    Scheduled {
        id: String,
        after: Duration,
    },
    Restarted {
        id: String,
    },
    RestartFailed {
        id: String,
        reason: String,
    },
    GaveUp {
        id: String,
    },
}

/// The watchdog's state between ticks.
#[derive(Default)]
pub struct Supervisor {
    policy: RestartPolicy,
    /// Crashed services waiting out their backoff, and when it ends.
    pending: HashMap<String, Instant>,
    given_up: HashSet<String>,
}

impl Supervisor {
    /// One pass over `services` at `now`. Starts are run inline, so this
    /// can block for as long as a start takes — call it off the UI thread.
    pub fn tick(&mut self, services: &[ServiceHandle], now: Instant) -> Vec<Event> {
        let mut events = Vec::new();
        let mut seen = HashSet::new();

        for service in services.iter().filter(|s| s.restarts_on_crash()) {
            let id = service.id().to_string();
            seen.insert(id.clone());

            if !service.crashed() {
                // Running again (by us or the user) or stopped on purpose —
                // nothing to do. Only a give-up is reset here: our own
                // successful restarts must keep counting against the budget,
                // or a crash loop would never be given up on.
                self.pending.remove(&id);
                if self.given_up.remove(&id) {
                    self.policy.forget(&id);
                }
                continue;
            }
            if self.given_up.contains(&id) {
                continue;
            }

            match self.pending.get(&id).copied() {
                None => match self.policy.on_crash(&id, now) {
                    Decision::RestartAfter(after) => {
                        self.pending.insert(id.clone(), now + after);
                        events.push(Event::Scheduled { id, after });
                    }
                    Decision::GiveUp => {
                        self.given_up.insert(id.clone());
                        events.push(Event::GaveUp { id });
                    }
                },
                Some(due) if due <= now => {
                    self.pending.remove(&id);
                    // A failed start leaves `crashed()` set, so the next
                    // tick schedules another attempt against the same budget.
                    match service.start() {
                        Ok(_) => events.push(Event::Restarted { id }),
                        Err(err) => events.push(Event::RestartFailed {
                            id,
                            reason: err.to_string(),
                        }),
                    }
                }
                Some(_) => {}
            }
        }

        // A pooled PHP version no project pins anymore is unregistered
        // entirely; nothing about it is worth keeping.
        self.pending.retain(|id, _| seen.contains(id));
        self.given_up.retain(|id| seen.contains(id));

        events
    }
}

/// Starts the watchdog on its own thread for the life of the app. Needs
/// `ServiceManager` already managed.
pub fn spawn(app: AppHandle) {
    let spawned = std::thread::Builder::new()
        .name("service-supervisor".to_string())
        .spawn(move || {
            let mut supervisor = Supervisor::default();
            loop {
                std::thread::sleep(TICK);
                let services = app.state::<ServiceManager>().handles();
                let events = supervisor.tick(&services, Instant::now());
                if events.is_empty() {
                    continue;
                }
                for event in &events {
                    report(&app, event);
                }
                if let Err(err) = app.emit(CHANGED_EVENT, ()) {
                    log::warn!("failed to emit {CHANGED_EVENT}: {err}");
                }
            }
        });
    if let Err(err) = spawned {
        log::error!("could not start the service supervisor: {err}");
    }
}

fn report(app: &AppHandle, event: &Event) {
    match event {
        Event::Scheduled { id, after } => {
            log::warn!("{id} exited unexpectedly — restarting in {after:?}")
        }
        Event::Restarted { id } => log::info!("{id} restarted after a crash"),
        Event::RestartFailed { id, reason } => {
            log::warn!("{id} could not be restarted after a crash: {reason}")
        }
        Event::GaveUp { id } => {
            log::error!(
                "{id} keeps crashing — automatic restart paused until it is started by hand"
            );
            notify_gave_up(app, id);
        }
    }
}

/// Same opt-in as the crash notification itself (`notify_on_crash`).
fn notify_gave_up(app: &AppHandle, id: &str) {
    let Some(settings) = app.try_state::<crate::config::settings::SettingsState>() else {
        return;
    };
    if !settings.0.lock().unwrap().notify_on_crash {
        return;
    }
    let name = app
        .state::<ServiceManager>()
        .find(id)
        .map(|service| service.info().name)
        .unwrap_or_else(|_| id.to_string());
    use tauri_plugin_notification::NotificationExt;
    if let Err(err) = app
        .notification()
        .builder()
        .title("Rezure")
        .body(format!(
            "{name} keeps crashing — automatic restart paused. Check its log, then start it again."
        ))
        .show()
    {
        log::warn!("failed to show give-up notification for {id}: {err}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::{Service, ServiceInfo, ServiceStatus};
    use crate::utils::error::AppError;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    struct FakeService {
        restartable: bool,
        crashed: AtomicBool,
        start_fails: AtomicBool,
        starts: AtomicUsize,
    }

    impl FakeService {
        fn new(restartable: bool) -> Arc<Self> {
            Arc::new(Self {
                restartable,
                crashed: AtomicBool::new(false),
                start_fails: AtomicBool::new(false),
                starts: AtomicUsize::new(0),
            })
        }

        fn crash(&self) {
            self.crashed.store(true, Ordering::SeqCst);
        }

        fn starts(&self) -> usize {
            self.starts.load(Ordering::SeqCst)
        }
    }

    impl Service for FakeService {
        fn id(&self) -> &str {
            "php"
        }
        fn info(&self) -> ServiceInfo {
            ServiceInfo {
                id: "php".to_string(),
                name: "PHP".to_string(),
                category: "Runtime".to_string(),
                status: ServiceStatus::Stopped,
                version: String::new(),
                port: 9000,
                cpu_percent: None,
                cpu_history: Vec::new(),
            }
        }
        fn start(&self) -> Result<ServiceInfo, AppError> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            if self.start_fails.load(Ordering::SeqCst) {
                return Err(AppError::Io("boom".to_string()));
            }
            self.crashed.store(false, Ordering::SeqCst);
            Ok(self.info())
        }
        fn stop(&self) -> Result<ServiceInfo, AppError> {
            self.crashed.store(false, Ordering::SeqCst);
            Ok(self.info())
        }
        fn crashed(&self) -> bool {
            self.crashed.load(Ordering::SeqCst)
        }
        fn restarts_on_crash(&self) -> bool {
            self.restartable
        }
    }

    fn handles(service: &Arc<FakeService>) -> Vec<ServiceHandle> {
        vec![service.clone() as ServiceHandle]
    }

    #[test]
    fn backoff_grows_then_gives_up_inside_the_window() {
        let mut policy = RestartPolicy::default();
        let t0 = Instant::now();
        for (i, delay) in BACKOFF.iter().enumerate() {
            let now = t0 + Duration::from_secs(i as u64);
            assert_eq!(policy.on_crash("php", now), Decision::RestartAfter(*delay));
        }
        assert_eq!(
            policy.on_crash("php", t0 + Duration::from_secs(10)),
            Decision::GiveUp
        );
    }

    #[test]
    fn old_crashes_stop_counting_once_outside_the_window() {
        let mut policy = RestartPolicy::default();
        let t0 = Instant::now();
        for _ in BACKOFF {
            policy.on_crash("php", t0);
        }
        let later = t0 + RESTART_WINDOW + Duration::from_secs(1);
        assert_eq!(
            policy.on_crash("php", later),
            Decision::RestartAfter(BACKOFF[0])
        );
    }

    #[test]
    fn a_crash_is_restarted_after_its_backoff() {
        let service = FakeService::new(true);
        let services = handles(&service);
        let mut supervisor = Supervisor::default();
        let t0 = Instant::now();

        service.crash();
        assert_eq!(
            supervisor.tick(&services, t0),
            vec![Event::Scheduled {
                id: "php".to_string(),
                after: BACKOFF[0]
            }]
        );
        // Not yet due.
        assert!(supervisor.tick(&services, t0).is_empty());
        assert_eq!(service.starts(), 0);

        assert_eq!(
            supervisor.tick(&services, t0 + BACKOFF[0]),
            vec![Event::Restarted {
                id: "php".to_string()
            }]
        );
        assert_eq!(service.starts(), 1);
        assert!(!service.crashed());
    }

    #[test]
    fn a_stop_during_backoff_cancels_the_restart() {
        let service = FakeService::new(true);
        let services = handles(&service);
        let mut supervisor = Supervisor::default();
        let t0 = Instant::now();

        service.crash();
        supervisor.tick(&services, t0);
        service.stop().unwrap();

        assert!(supervisor.tick(&services, t0 + BACKOFF[0]).is_empty());
        assert_eq!(service.starts(), 0);
    }

    #[test]
    fn services_that_do_not_opt_in_are_left_alone() {
        let service = FakeService::new(false);
        let services = handles(&service);
        let mut supervisor = Supervisor::default();

        service.crash();
        let t0 = Instant::now();
        assert!(supervisor.tick(&services, t0).is_empty());
        assert!(supervisor
            .tick(&services, t0 + Duration::from_secs(60))
            .is_empty());
        assert_eq!(service.starts(), 0);
    }

    #[test]
    fn a_crash_loop_is_given_up_on_until_the_user_steps_in() {
        let service = FakeService::new(true);
        let services = handles(&service);
        let mut supervisor = Supervisor::default();
        let mut now = Instant::now();

        for delay in BACKOFF {
            service.crash();
            supervisor.tick(&services, now);
            now += delay;
            supervisor.tick(&services, now);
        }
        assert_eq!(service.starts(), BACKOFF.len());

        service.crash();
        assert_eq!(
            supervisor.tick(&services, now),
            vec![Event::GaveUp {
                id: "php".to_string()
            }]
        );
        assert!(supervisor
            .tick(&services, now + Duration::from_secs(60))
            .is_empty());
        assert_eq!(service.starts(), BACKOFF.len());

        // The user starts it by hand: the budget resets, so the next crash
        // is handled from the shortest delay again.
        service.start().unwrap();
        supervisor.tick(&services, now);
        service.crash();
        assert_eq!(
            supervisor.tick(&services, now),
            vec![Event::Scheduled {
                id: "php".to_string(),
                after: BACKOFF[0]
            }]
        );
    }

    #[test]
    fn a_failed_restart_is_retried_with_a_longer_delay() {
        let service = FakeService::new(true);
        let services = handles(&service);
        let mut supervisor = Supervisor::default();
        let t0 = Instant::now();

        service.crash();
        service.start_fails.store(true, Ordering::SeqCst);
        supervisor.tick(&services, t0);
        let events = supervisor.tick(&services, t0 + BACKOFF[0]);
        assert!(matches!(events.as_slice(), [Event::RestartFailed { .. }]));

        assert_eq!(
            supervisor.tick(&services, t0 + BACKOFF[0]),
            vec![Event::Scheduled {
                id: "php".to_string(),
                after: BACKOFF[1]
            }]
        );
    }
}
