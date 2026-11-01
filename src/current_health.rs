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
    /// Local commits are landing while publication is still pending. This is
    /// progress, but explicitly NOT converged/successful updating.
    Publishing,
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
    let pending = row.modified + row.staged + row.untracked > 0
        || row.ahead > 0
        || row.behind > 0
        || row.state_flags.iter().any(|flag| flag == "HYGIENE_PENDING");
    let blocked = row.pack_too_large
        || row.missing_objects > 0
        || row.push_status == "BROKEN"
        || row
            .state_flags
            .iter()
            .any(|flag| flag.starts_with("EXCLUDED_RECOVERY") || flag == "HYGIENE_INSPECT_FAILED");
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
    let phase = row.health.scheduler.as_ref().and_then(|e| e.phase);
    // Local work can continue after the publication budget is exhausted. An
    // active committer/queue is not evidence that this backlog can be pushed.
    // Preserve an actual in-flight publication or an explicit current blocker;
    // otherwise expose the exhausted budget while retaining scheduler details.
    if row.ahead > 0
        && row.push_status == "PUSH_STUCK"
        && !matches!(phase, Some(Phase::Pushing | Phase::Blocked))
    {
        return Current::Stalled;
    }
    if let Some(phase) = phase {
        match phase {
            Phase::Pushing => return Current::Publishing,
            Phase::Committing => return Current::Working,
            Phase::Working => return Current::Working,
            // A pending probe has no execution stamp. Its reservation is
            // useful evidence, but cannot establish actively running work.
            Phase::Queued | Phase::Inspecting => return Current::Queued,
            Phase::Cancelling | Phase::Retrying => return Current::Retrying,
            Phase::Blocked => return Current::Blocked,
        }
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

// Status labels are named once and reused by `status()`, the layout width
// calculation and the drift test. Hand-tuned column widths had already
// drifted twice: the STATUS cell was `Fixed(13)` while `📤 PUBLISHING` is
// exactly 13 display columns, so the rich table wrapped the word
// PUBLISHING onto a second line and the row looked like two rows.
pub(super) const LABEL_CLEAN: &str = "✅ CLEAN";
pub(super) const LABEL_EXCLUDED: &str = "🚫 EXCLUDED";
pub(super) const LABEL_QUEUED: &str = "🟡 QUEUED";
pub(super) const LABEL_WORKING: &str = "🔄 WORKING";
pub(super) const LABEL_PUBLISHING: &str = "📤 PUBLISHING";
pub(super) const LABEL_RETRYING: &str = "⏳ RETRYING";
pub(super) const LABEL_STALLED: &str = "🔴 STALLED";
pub(super) const LABEL_BLOCKED: &str = "🚫 BLOCKED";
pub(super) const LABEL_PAUSED: &str = "⏸️ PAUSED";
pub(super) const LABEL_UNKNOWN: &str = "❔ UNKNOWN";

/// Every label `status()` can return. Single source for layout sizing.
pub(crate) const ALL_STATUS_LABELS: [&str; 10] = [
    LABEL_CLEAN,
    LABEL_EXCLUDED,
    LABEL_QUEUED,
    LABEL_WORKING,
    LABEL_PUBLISHING,
    LABEL_RETRYING,
    LABEL_STALLED,
    LABEL_BLOCKED,
    LABEL_PAUSED,
    LABEL_UNKNOWN,
];

pub(super) fn status(row: &RepoReportRow) -> (&'static str, Color) {
    match classify(row) {
        Current::Converged => (LABEL_CLEAN, Color::Green),
        Current::Excluded => (LABEL_EXCLUDED, Color::DarkGrey),
        Current::Queued => (LABEL_QUEUED, Color::Cyan),
        Current::Working => (LABEL_WORKING, Color::Cyan),
        // Named separately from WORKING: the tip is not on the remotes yet.
        Current::Publishing => (LABEL_PUBLISHING, Color::Cyan),
        Current::Retrying => (LABEL_RETRYING, Color::Yellow),
        Current::Stalled => (LABEL_STALLED, Color::Red),
        Current::Blocked => (LABEL_BLOCKED, Color::Red),
        Current::Paused => (LABEL_PAUSED, Color::Yellow),
        Current::Unknown => (LABEL_UNKNOWN, Color::Yellow),
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
