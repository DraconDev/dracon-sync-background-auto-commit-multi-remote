//! Current convergence and historical observations are independent axes.
#[cfg(test)]
#[path = "current_health_tests.rs"]
mod tests;
use super::{Color, RepoReportRow, StateCause};
use crate::daemon::{
    push_history::Observation,
    scheduler_evidence::{Phase, RepoEvidence},
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Current {
    Converged,
    Excluded,
    Queued,
    Working,
    Retrying,
    Stalled,
    Blocked,
    Paused,
    #[default]
    Unknown,
}
#[derive(Clone, Debug, Default, Serialize)]
pub(super) struct Evidence {
    pub current: Current,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheduler: Option<RepoEvidence>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_push: Option<Observation>,
}

pub(super) fn classify(row: &RepoReportRow) -> Current {
    let pending = row.modified + row.staged + row.untracked > 0 || row.ahead > 0 || row.behind > 0;
    let blocked = row.pack_too_large || row.missing_objects > 0 || row.push_status == "BROKEN";
    // Legacy STUCK_PULL means merely behind-with-upstream, not an actual
    // pull blocker. A current blocker requires affirmative scheduler evidence.
    if blocked {
        return Current::Blocked;
    }
    if row.push_status == "BLOCKED" || matches!(row.state_cause, StateCause::Unowned { .. }) {
        return Current::Excluded;
    }
    // Historical errors and ancient commits cannot make unchanged idle work
    // unhealthy. Excluded bytes remain explicitly outside daemon convergence.
    if !pending {
        return if row.excluded_dirty > 0 {
            Current::Excluded
        } else {
            Current::Converged
        };
    }
    if row.frozen_secs.is_some() || row.health.mode.as_deref() == Some("paused") {
        return Current::Paused;
    }
    if let Some(phase) = row.health.scheduler.as_ref().and_then(|e| e.phase) {
        match phase {
            Phase::Working => return Current::Working,
            // A pending probe has no execution stamp. Its reservation is
            // useful evidence, but cannot establish actively running work.
            Phase::Queued | Phase::Inspecting => return Current::Queued,
            Phase::Cancelling | Phase::Retrying => return Current::Retrying,
            Phase::Blocked => return Current::Blocked,
        }
    }
    // Only PUSH_STUCK is produced by explicit exhausted-budget evidence.
    // Legacy STUCK/FAIL can mean a single recent failure with retries left.
    if row.ahead > 0 && row.push_status == "PUSH_STUCK" {
        return Current::Stalled;
    }
    if row.push_status == "INTENTIONAL" {
        return Current::Excluded;
    }
    if let Some(evidence) = &row.health.scheduler {
        // Continuous eligible dirt + no current gate and no recent admission
        // establish scheduler starvation, not a guessed operation heartbeat.
        if evidence.eligible
            && evidence.dirty_secs.is_some_and(|age| age >= 900)
            && evidence.admitted_secs.is_none_or(|age| age >= 900)
        {
            return Current::Stalled;
        }
        if evidence.eligible {
            return Current::Queued;
        }
    }
    Current::Unknown
}

pub(super) fn status(row: &RepoReportRow) -> (&'static str, Color) {
    match classify(row) {
        Current::Converged => ("✅ CLEAN", Color::Green),
        Current::Excluded => ("🚫 EXCLUDED", Color::DarkGrey),
        Current::Queued => ("🟡 QUEUED", Color::Cyan),
        Current::Working => ("🔄 WORKING", Color::Cyan),
        Current::Retrying => ("⏳ RETRYING", Color::Yellow),
        Current::Stalled => ("🔴 STALLED", Color::Red),
        Current::Blocked => ("🚫 BLOCKED", Color::Red),
        Current::Paused => ("⏸️ PAUSED", Color::Yellow),
        Current::Unknown => ("❔ UNKNOWN", Color::Yellow),
    }
}
/// Rich tables have dedicated ahead and excluded columns. Keep the entire
/// state visible rather than clipping a duplicate count suffix.
pub(super) fn rich_activity(row: &RepoReportRow) -> &'static str {
    if classify(row) == Current::Converged {
        "⚪ idle"
    } else {
        status(row).0
    }
}

pub(super) fn label(row: &RepoReportRow) -> String {
    let current = classify(row);
    let mut label = rich_activity(row).to_owned();
    if row.ahead > 0 {
        label.push_str(&format!(" ({} ahead)", row.ahead));
    }
    if row.excluded_dirty > 0 && current != Current::Excluded {
        label.push_str(&format!(" · {} excl", row.excluded_dirty));
    }
    label
}
pub(super) fn push(row: &RepoReportRow) -> (String, Color) {
    match &row.health.last_push {
        Some(observed) => {
            let age = super::timestamp_secs().saturating_sub(observed.attempted_at);
            let age = if age < 60 {
                format!("{age}s")
            } else {
                super::shorten_mins(age / 60)
            };
            if observed.succeeded {
                (format!("✅ OK {age}"), Color::Green)
            } else {
                (format!("❌ FAIL {age}"), Color::Red)
            }
        }
        None => ("— unknown".into(), Color::DarkGrey),
    }
}
