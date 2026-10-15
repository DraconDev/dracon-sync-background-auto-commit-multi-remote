use super::*;

#[test]
fn current_convergence_output_table_is_independent_of_push_history_and_commit_age() {
    // name, dirty, ahead, phase, eligible dirty age, admission age, excluded,
    // legacy dispatch status, expected current state, visible label.
    let cases = [
        (
            "queued",
            true,
            0,
            Some(Phase::Queued),
            None,
            None,
            0,
            "OK",
            Current::Queued,
            "QUEUED",
        ),
        (
            "started",
            true,
            1,
            Some(Phase::Working),
            Some(5000),
            None,
            0,
            "PENDING",
            Current::Working,
            "WORKING",
        ),
        (
            "checking",
            true,
            0,
            Some(Phase::Inspecting),
            None,
            None,
            0,
            "OK",
            Current::Queued,
            "QUEUED",
        ),
        (
            "retry",
            true,
            1,
            Some(Phase::Retrying),
            Some(5000),
            None,
            0,
            "PENDING",
            Current::Retrying,
            "RETRYING",
        ),
        (
            "cancelling",
            true,
            0,
            Some(Phase::Cancelling),
            Some(5000),
            None,
            0,
            "OK",
            Current::Retrying,
            "RETRYING",
        ),
        (
            "excluded",
            false,
            0,
            None,
            None,
            None,
            3,
            "OK",
            Current::Excluded,
            "EXCLUDED",
        ),
        (
            "starved",
            true,
            0,
            None,
            Some(900),
            None,
            0,
            "OK",
            Current::Stalled,
            "STALLED",
        ),
        (
            "recent admission",
            true,
            0,
            None,
            Some(5000),
            Some(10),
            0,
            "OK",
            Current::Queued,
            "QUEUED",
        ),
        (
            "new edits old commit",
            true,
            0,
            None,
            None,
            None,
            0,
            "OK",
            Current::Unknown,
            "UNKNOWN",
        ),
        (
            "recent failure with retry budget remaining",
            false,
            1,
            None,
            None,
            None,
            0,
            "STUCK",
            Current::Unknown,
            "UNKNOWN",
        ),
        (
            "legacy failure without scheduler evidence",
            false,
            1,
            None,
            None,
            None,
            0,
            "FAIL",
            Current::Unknown,
            "UNKNOWN",
        ),
        (
            "budget exhausted",
            false,
            1,
            None,
            None,
            None,
            0,
            "PUSH_STUCK",
            Current::Stalled,
            "STALLED",
        ),
        (
            "idle old failure",
            false,
            0,
            None,
            None,
            None,
            0,
            "FAIL",
            Current::Converged,
            "CLEAN",
        ),
        (
            "idle old budget",
            false,
            0,
            None,
            None,
            None,
            0,
            "PUSH_STUCK",
            Current::Converged,
            "CLEAN",
        ),
    ];
    for (name, dirty, ahead, phase, dirty_age, admitted, excluded, dispatch, expected, visible) in
        cases
    {
        for succeeded in [true, false] {
            let mut row = RepoReportRow::for_tests("/fixture");
            row.modified = usize::from(dirty);
            row.ahead = ahead;
            row.excluded_dirty = excluded;
            row.push_status = dispatch.into();
            row.last_when = "90 days ago".into();
            row.state_cause = StateCause::Stalled; // Deliberately misleading old inference.
            row.health.scheduler =
                (phase.is_some() || dirty_age.is_some()).then_some(RepoEvidence {
                    phase,
                    dirty_secs: dirty_age,
                    admitted_secs: admitted,
                    eligible: dirty_age.is_some(),
                    ..Default::default()
                });
            row.health.last_push = Some(Observation {
                version: 1,
                repo: row.repo.clone(),
                attempted_at: super::super::timestamp_secs() - 600,
                succeeded,
            });
            assert_eq!(classify(&row), expected, "{name}, history={succeeded}");
            assert!(
                super::super::status_pair(&row).0.contains(visible),
                "{name}"
            );
            assert!(
                label(&row).contains(if expected == Current::Converged {
                    "idle"
                } else {
                    visible
                }),
                "{name}"
            );
            assert!(push(&row).0.contains(if succeeded { "OK" } else { "FAIL" }));
            // JSON exposes the same typed current state as human output.
            row.health.current = classify(&row);
            let json = serde_json::to_value(&row).unwrap();
            assert_eq!(
                json["health"]["current"],
                serde_json::to_value(expected).unwrap()
            );
            assert_eq!(json["health"]["last_push"]["succeeded"], succeeded);
            let summary = super::super::summary_what(&row, 200);
            assert!(
                summary.contains(if succeeded {
                    "last push: ✅ OK"
                } else {
                    "last push: ❌ FAIL"
                }),
                "summary must name historical results: {summary}"
            );
            let table =
                super::super::build_repos_rich_table(std::slice::from_ref(&row), false, 200)
                    .to_string();
            assert!(table.contains(visible), "actual table for {name}: {table}");
            assert!(
                table.contains(if succeeded { "✅ OK" } else { "❌ FAIL" }),
                "history and convergence must both remain visible: {table}"
            );
        }
    }
}

#[test]
fn behind_counts_do_not_prove_a_pull_blocker() {
    for (phase, expected, visible) in [
        (None, Current::Unknown, "UNKNOWN"),
        (Some(Phase::Working), Current::Working, "WORKING"),
        (Some(Phase::Queued), Current::Queued, "QUEUED"),
        (Some(Phase::Retrying), Current::Retrying, "RETRYING"),
        (Some(Phase::Blocked), Current::Blocked, "BLOCKED"),
    ] {
        let mut row = RepoReportRow::for_tests("/fixture");
        row.behind = 2;
        row.state_flags.push("STUCK_PULL".into());
        row.health.scheduler = phase.map(|phase| RepoEvidence {
            phase: Some(phase),
            ..Default::default()
        });
        assert_eq!(classify(&row), expected, "phase={phase:?}");
        row.health.current = classify(&row);
        let json = serde_json::to_value(&row).unwrap();
        assert_eq!(
            json["health"]["current"],
            serde_json::to_value(expected).unwrap()
        );
        let table = super::super::build_repos_rich_table(std::slice::from_ref(&row), false, 200)
            .to_string();
        assert!(table.contains(visible), "phase={phase:?}: {table}");
        // Real history/pack safety guards must still win over started work.
        row.pack_too_large = true;
        assert_eq!(classify(&row), Current::Blocked);
    }
}

#[test]
fn current_convergence_freeze_block_and_missing_history_are_explicit() {
    let mut row = RepoReportRow::for_tests("/fixture");
    row.modified = 1;
    row.frozen_secs = Some(5000);
    assert_eq!(classify(&row), Current::Paused);
    row.modified = 0;
    assert_eq!(
        classify(&row),
        Current::Converged,
        "frozen idle is not unhealthy"
    );
    row.push_status = "BROKEN".into();
    assert_eq!(classify(&row), Current::Blocked);
    row.push_status = "BLOCKED".into();
    assert_eq!(
        classify(&row),
        Current::Excluded,
        "ownership opt-out is not a fake live worker"
    );
    assert_eq!(
        push(&row).0,
        "— unknown",
        "an old remote commit is not push evidence"
    );
}
