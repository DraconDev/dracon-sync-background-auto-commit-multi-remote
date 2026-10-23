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
            // Publication is distinct from success: the tip is not on the
            // remotes yet, so the label must never read CLEAN/WORKING.
            "publishing",
            false,
            3,
            Some(Phase::Pushing),
            Some(5000),
            None,
            0,
            "PENDING",
            Current::Publishing,
            "PUBLISHING",
        ),
        (
            "committing",
            true,
            1,
            Some(Phase::Committing),
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
fn exhausted_publication_budget_is_not_hidden_by_local_commit_activity() {
    use crate::daemon::scheduler_evidence::Snapshot;
    for phase in [None, Some(Phase::Working), Some(Phase::Committing), Some(Phase::Queued),
        Some(Phase::Inspecting), Some(Phase::Retrying), Some(Phase::Pushing), Some(Phase::Blocked)] {
        let mut row = RepoReportRow::for_tests("/fixture/continuously-edited");
        row.modified = 1;
        row.ahead = 694;
        row.push_status = "PUSH_STUCK".into();
        let snapshot = Snapshot {
            version: 1,
            written_at: 100,
            mode: "running".into(),
            repos: [(std::path::PathBuf::from(&row.repo), RepoEvidence {
                phase, eligible: false, ..Default::default()
            })].into(),
        };
        let mut rows = [row];
        super::super::apply_report_scheduler_snapshot(&mut rows, Some(&snapshot), 100);
        let row = &rows[0];
        let expected = match phase {
            Some(Phase::Pushing) => Current::Publishing,
            Some(Phase::Blocked) => Current::Blocked,
            _ => Current::Stalled,
        };
        assert_eq!(row.health.current, expected, "phase={phase:?}");
        assert_eq!(row.active, expected == Current::Publishing);
        assert_eq!(row.warn, expected != Current::Publishing);
        assert_eq!(row.health.scheduler.as_ref().unwrap().phase, phase,
            "retain the separate actor evidence rather than hiding local work");
        let output = serde_json::to_value(row).unwrap();
        assert_eq!(output["warn"], expected != Current::Publishing);
        assert_eq!(output["health"]["current"], serde_json::to_value(expected).unwrap());
    }
}

#[test]
fn rich_rendered_states_keep_complete_activity_and_single_line_rows() {
    let states = [
        Current::Converged,
        Current::Excluded,
        Current::Queued,
        Current::Working,
        Current::Retrying,
        Current::Stalled,
        Current::Blocked,
        Current::Paused,
        Current::Unknown,
    ];
    let mut rows = Vec::new();
    for state in states {
        let mut row = RepoReportRow::for_tests(&format!(
            "/fixture/長い名前-👩‍💻-e\u{301}-{state:?}-very-long-repository"
        ));
        row.modified = usize::from(state != Current::Converged);
        row.ahead = usize::from(state == Current::Working);
        row.last_author = "作者👩‍💻".into();
        row.commits_24h = 12345;
        row.hint = format!("reason for {state:?}");
        match state {
            Current::Excluded => row.push_status = "BLOCKED".into(),
            Current::Queued | Current::Working | Current::Retrying | Current::Blocked => {
                let phase = match state {
                    Current::Queued => Phase::Queued,
                    Current::Working => Phase::Working,
                    Current::Retrying => Phase::Retrying,
                    _ => Phase::Blocked,
                };
                row.health.scheduler = Some(RepoEvidence {
                    phase: Some(phase),
                    ..Default::default()
                });
            }
            Current::Stalled => {
                row.health.scheduler = Some(RepoEvidence {
                    eligible: true,
                    dirty_secs: Some(900),
                    ..Default::default()
                })
            }
            Current::Paused => row.frozen_secs = Some(1),
            _ => {}
        }
        assert_eq!(classify(&row), state);
        rows.push(row);
    }
    for width in [165, 200, 240, 320] {
        let output = super::super::build_repos_rich_table(&rows, false, width).to_string();
        assert_eq!(
            output.lines().count(),
            rows.len() + 4,
            "wrapped at {width}: {output}"
        );
        for line in output.lines() {
            // Measure the text, not the escape codes: comfy-table styles the
            // table with ANSI colours when stdout is a terminal, and the
            // escapes occupy no columns on screen. The Nix build's stdout
            // is styled, a piped test run is not, and the width must be the
            // same either way.
            let visible = crate::visibility::strip_ansi(line);
            assert_eq!(
                unicode_width::UnicodeWidthStr::width(visible.as_str()),
                usize::from(width),
                "{line}"
            );
        }
        for row in &rows {
            let label = rich_activity(row);
            // Both STATUS and ACTIVITY carry the COMPLETE state; mere global
            // substring presence used to conceal truncated ACTIVITY cells.
            let expected = if classify(row) == Current::Converged {
                1
            } else {
                2
            };
            assert_eq!(
                output.matches(label).count(),
                expected,
                "label {label}: {output}"
            );
        }
        assert!(!output.contains("WORKING ("));
        println!("=== rich renderer {width} columns ===\n{output}");
    }
    // Forced rich below its supported floor must remain non-panicking.
    let narrow = super::super::build_repos_rich_table(&rows, false, 80).to_string();
    assert!(narrow.contains("STATUS") && narrow.contains("ACTIVITY"));
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
