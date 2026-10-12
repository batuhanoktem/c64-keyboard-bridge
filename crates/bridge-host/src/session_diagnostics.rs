//! Bounded runtime evidence. No peer text, credentials or keyboard data belong here.
use serde::{Deserialize, Serialize};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    #[default]
    Idle,
    Hello,
    Synchronizing,
    Active,
    Closing,
}
impl SessionPhase {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Hello => "hello",
            Self::Synchronizing => "synchronizing",
            Self::Active => "active",
            Self::Closing => "closing",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FailureReason {
    HelloTimeout,
    SyncTimeout,
    HeartbeatTimeout,
    WriteTimeout,
    Transport,
    Authentication,
    Protocol,
    QueueOverflow,
    Input,
    Repeat,
    Layout,
    Cleanup,
}
impl FailureReason {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::HelloTimeout => "hello_timeout",
            Self::SyncTimeout => "sync_timeout",
            Self::HeartbeatTimeout => "heartbeat_timeout",
            Self::WriteTimeout => "write_timeout",
            Self::Transport => "transport",
            Self::Authentication => "authentication",
            Self::Protocol => "protocol",
            Self::QueueOverflow => "queue_overflow",
            Self::Input => "input",
            Self::Repeat => "repeat",
            Self::Layout => "layout",
            Self::Cleanup => "cleanup",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CleanupOutcome {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct FailureCounters {
    pub hello_timeout: u64,
    pub sync_timeout: u64,
    pub heartbeat_timeout: u64,
    pub write_timeout: u64,
    pub transport: u64,
    pub authentication: u64,
    pub protocol: u64,
    pub queue_overflow: u64,
    pub input: u64,
    pub repeat: u64,
    pub layout: u64,
    pub cleanup: u64,
}
impl FailureCounters {
    fn increment(&mut self, reason: FailureReason) {
        let counter = match reason {
            FailureReason::HelloTimeout => &mut self.hello_timeout,
            FailureReason::SyncTimeout => &mut self.sync_timeout,
            FailureReason::HeartbeatTimeout => &mut self.heartbeat_timeout,
            FailureReason::WriteTimeout => &mut self.write_timeout,
            FailureReason::Transport => &mut self.transport,
            FailureReason::Authentication => &mut self.authentication,
            FailureReason::Protocol => &mut self.protocol,
            FailureReason::QueueOverflow => &mut self.queue_overflow,
            FailureReason::Input => &mut self.input,
            FailureReason::Repeat => &mut self.repeat,
            FailureReason::Layout => &mut self.layout,
            FailureReason::Cleanup => &mut self.cleanup,
        };
        *counter = counter.saturating_add(1);
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CurrentSession {
    pub generation: u64,
    pub phase: SessionPhase,
    pub valid_frame_age_ms: Option<u64>,
    pub validated_frames: u64,
    #[serde(skip)]
    started: Option<Instant>,
    #[serde(skip)]
    last_valid: Option<Instant>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct FailureRecord {
    /// Correlates this failure with its asynchronous worker cleanup.
    pub generation: u64,
    pub session_generation: u64,
    pub reason: FailureReason,
    pub phase: SessionPhase,
    pub elapsed_ms: u64,
    pub valid_frame_age_ms: Option<u64>,
    pub validated_frames: u64,
    pub cleanup: CleanupOutcome,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SessionDiagnostics {
    pub attempts: u64,
    pub authenticated_sessions: u64,
    pub reconnects: u64,
    pub failures: FailureCounters,
    pub current: CurrentSession,
    pub last_failure: Option<FailureRecord>,
    #[serde(skip)]
    next_failure_generation: u64,
}
fn elapsed_ms(start: Instant) -> u64 {
    start.elapsed().as_millis().min(u64::MAX as u128) as u64
}
impl SessionDiagnostics {
    pub(crate) fn begin(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
        self.current = CurrentSession {
            generation: self.current.generation.wrapping_add(1),
            phase: SessionPhase::Hello,
            started: Some(Instant::now()),
            ..CurrentSession::default()
        };
    }
    pub(crate) fn authenticated(&mut self) {
        if self.authenticated_sessions != 0 {
            self.reconnects = self.reconnects.saturating_add(1);
        }
        self.authenticated_sessions = self.authenticated_sessions.saturating_add(1);
        self.current.phase = SessionPhase::Synchronizing;
    }
    pub(crate) fn phase(&mut self, phase: SessionPhase) {
        self.current.phase = phase;
    }
    pub(crate) fn valid_frame(&mut self) {
        self.current.validated_frames = self.current.validated_frames.saturating_add(1);
        self.current.last_valid = Some(Instant::now());
        self.current.valid_frame_age_ms = Some(0);
    }
    pub(crate) fn refresh(&mut self) {
        self.current.valid_frame_age_ms = self.current.last_valid.map(elapsed_ms);
    }
    pub(crate) fn record_failure(&mut self, reason: FailureReason, cleanup: CleanupOutcome) -> u64 {
        self.refresh();
        self.record_context_failure(reason, cleanup, &self.current.clone())
    }
    fn count_failure(&mut self, reason: FailureReason, cleanup: CleanupOutcome) {
        self.failures.increment(reason);
        if cleanup == CleanupOutcome::Failed && reason != FailureReason::Cleanup {
            self.failures.increment(FailureReason::Cleanup);
        }
    }
    fn record_context_failure(
        &mut self,
        reason: FailureReason,
        cleanup: CleanupOutcome,
        context: &CurrentSession,
    ) -> u64 {
        self.count_failure(reason, cleanup);
        self.next_failure_generation = self.next_failure_generation.wrapping_add(1);
        self.last_failure = Some(FailureRecord {
            generation: self.next_failure_generation,
            session_generation: context.generation,
            reason,
            phase: context.phase,
            elapsed_ms: context.started.map(elapsed_ms).unwrap_or(0),
            valid_frame_age_ms: context.last_valid.map(elapsed_ms),
            validated_frames: context.validated_frames,
            cleanup,
        });
        self.next_failure_generation
    }
    /// Native work can finish after a transport session has ended or reconnected.
    /// Count those faults without attributing them to the replacement session or
    /// replacing evidence from the same or a newer session. An older retained
    /// record can be replaced using the work's original context.
    pub(crate) fn record_worker_failure(
        &mut self,
        reason: FailureReason,
        cleanup: CleanupOutcome,
        context: &CurrentSession,
    ) {
        if context.generation == self.current.generation
            && (self.current.phase != SessionPhase::Idle || context.phase == SessionPhase::Idle)
        {
            self.record_failure(reason, cleanup);
        } else if self.last_failure.as_ref().is_none_or(|failure| {
            context.generation != failure.session_generation
                && context.generation.wrapping_sub(failure.session_generation) < (1u64 << 63)
        }) {
            self.record_context_failure(reason, cleanup, context);
        } else {
            self.count_failure(reason, cleanup);
        }
    }
    pub(crate) fn complete_cleanup(&mut self, generation: u64, succeeded: bool) {
        if !succeeded {
            self.failures.increment(FailureReason::Cleanup);
        }
        if let Some(failure) = &mut self.last_failure
            && failure.generation == generation
            && failure.cleanup == CleanupOutcome::Pending
        {
            failure.cleanup = if succeeded {
                CleanupOutcome::Succeeded
            } else {
                CleanupOutcome::Failed
            };
        }
    }
    pub(crate) fn end(&mut self) {
        self.current = CurrentSession {
            generation: self.current.generation,
            ..CurrentSession::default()
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counters_saturate_and_old_cleanup_cannot_change_newer_evidence() {
        let mut diagnostics = SessionDiagnostics {
            attempts: u64::MAX,
            authenticated_sessions: u64::MAX,
            reconnects: u64::MAX,
            ..SessionDiagnostics::default()
        };
        diagnostics.failures.protocol = u64::MAX;
        diagnostics.begin();
        diagnostics.authenticated();
        diagnostics.valid_frame();
        let older = diagnostics.record_failure(FailureReason::Protocol, CleanupOutcome::Pending);
        let newer = diagnostics.record_failure(FailureReason::Transport, CleanupOutcome::Pending);
        diagnostics.complete_cleanup(older, true);
        assert_eq!(
            diagnostics.last_failure.as_ref().unwrap().cleanup,
            CleanupOutcome::Pending
        );
        diagnostics.complete_cleanup(newer, false);
        assert_eq!(
            diagnostics.last_failure.as_ref().unwrap().cleanup,
            CleanupOutcome::Failed
        );
        diagnostics.complete_cleanup(newer, true);
        assert_eq!(
            diagnostics.last_failure.as_ref().unwrap().cleanup,
            CleanupOutcome::Failed
        );
        assert_eq!(diagnostics.attempts, u64::MAX);
        assert_eq!(diagnostics.authenticated_sessions, u64::MAX);
        assert_eq!(diagnostics.reconnects, u64::MAX);
        assert_eq!(diagnostics.failures.protocol, u64::MAX);
        assert_eq!(diagnostics.failures.cleanup, 1);
    }

    #[test]
    fn stale_worker_faults_preserve_evidence_but_new_idle_controls_can_replace_it() {
        let mut diagnostics = SessionDiagnostics::default();
        diagnostics.begin();
        diagnostics.authenticated();
        diagnostics.phase(SessionPhase::Active);
        diagnostics.valid_frame();
        let old_work = diagnostics.current.clone();
        let failure =
            diagnostics.record_failure(FailureReason::HeartbeatTimeout, CleanupOutcome::Pending);
        let expected = diagnostics.last_failure.clone();
        diagnostics.end();
        diagnostics.record_worker_failure(FailureReason::Input, CleanupOutcome::Failed, &old_work);
        assert_eq!(diagnostics.last_failure, expected);
        diagnostics.begin();
        diagnostics.record_worker_failure(FailureReason::Repeat, CleanupOutcome::Failed, &old_work);
        assert_eq!(diagnostics.last_failure, expected);
        assert_eq!(diagnostics.failures.input, 1);
        assert_eq!(diagnostics.failures.repeat, 1);
        assert_eq!(diagnostics.failures.cleanup, 2);
        diagnostics.complete_cleanup(failure, true);
        assert_eq!(
            diagnostics.last_failure.as_ref().unwrap().cleanup,
            CleanupOutcome::Succeeded
        );

        diagnostics.end();
        let new_idle_control = diagnostics.current.clone();
        diagnostics.record_worker_failure(
            FailureReason::Layout,
            CleanupOutcome::Succeeded,
            &new_idle_control,
        );
        let newer = diagnostics.last_failure.as_ref().unwrap();
        assert_eq!(newer.reason, FailureReason::Layout);
        assert_eq!(newer.phase, SessionPhase::Idle);
        assert_eq!(newer.session_generation, 2);
        diagnostics.complete_cleanup(failure, false);
        assert_eq!(
            diagnostics.last_failure.as_ref().unwrap().reason,
            FailureReason::Layout
        );
        assert_eq!(
            diagnostics.last_failure.as_ref().unwrap().cleanup,
            CleanupOutcome::Succeeded
        );
    }

    #[test]
    fn newer_stale_fault_replaces_older_record_with_its_original_session_context() {
        let mut diagnostics = SessionDiagnostics::default();
        diagnostics.begin();
        diagnostics.record_failure(FailureReason::Authentication, CleanupOutcome::Succeeded);
        diagnostics.end();
        diagnostics.begin();
        diagnostics.authenticated();
        diagnostics.phase(SessionPhase::Active);
        diagnostics.valid_frame();
        let older_work = diagnostics.current.clone();
        diagnostics.end();
        diagnostics.begin();
        diagnostics.record_worker_failure(
            FailureReason::Repeat,
            CleanupOutcome::Failed,
            &older_work,
        );
        let record = diagnostics.last_failure.as_ref().unwrap();
        assert_eq!(record.reason, FailureReason::Repeat);
        assert_eq!(record.session_generation, 2);
        assert_eq!(record.phase, SessionPhase::Active);
        assert_eq!(record.validated_frames, 1);
        assert_eq!(diagnostics.current.generation, 3);
        assert_eq!(diagnostics.current.phase, SessionPhase::Hello);
    }

    #[tokio::test(start_paused = true)]
    async fn age_and_failure_elapsed_use_monotonic_valid_frames() {
        let mut diagnostics = SessionDiagnostics::default();
        diagnostics.begin();
        tokio::time::advance(std::time::Duration::from_millis(10)).await;
        diagnostics.valid_frame();
        tokio::time::advance(std::time::Duration::from_millis(20)).await;
        diagnostics.refresh();
        assert_eq!(diagnostics.current.valid_frame_age_ms, Some(20));
        diagnostics.record_failure(FailureReason::SyncTimeout, CleanupOutcome::Pending);
        diagnostics.end();
        let failure = diagnostics.last_failure.as_ref().unwrap();
        assert_eq!(failure.elapsed_ms, 30);
        assert_eq!(failure.validated_frames, 1);
        assert_eq!(failure.valid_frame_age_ms, Some(20));
        assert_eq!(diagnostics.current.phase, SessionPhase::Idle);
        assert_eq!(diagnostics.current.valid_frame_age_ms, None);
        let bytes = serde_json::to_vec(&diagnostics).unwrap();
        assert!(bytes.len() < 1500);
        let decoded: SessionDiagnostics = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(decoded.last_failure.unwrap().elapsed_ms, 30);
        assert_eq!(
            serde_json::from_str::<SessionDiagnostics>("{}")
                .unwrap()
                .attempts,
            0
        );
    }
}
