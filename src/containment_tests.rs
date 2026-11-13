use super::*;

#[test]
fn ledger_prunes_old_and_future_entries_and_counts_the_window() {
    let now = 1_000_000;
    let existing = format!("{}\n{}\nnot-a-number\n{}\n{}\n", now - 90_000, now - 4_000, now - 100, now + 50);
    let next = ledger_with_start(&existing, now);
    // >24h old and future-dated entries are dropped; the new start is appended.
    assert_eq!(next, format!("{}\n{}\n{}\n", now - 4_000, now - 100, now));
    assert_eq!(starts_within(&next, now, RESTART_WINDOW_SECS), 2);
}

#[test]
fn ledger_is_capped() {
    let now = 100_000;
    let existing: String = (0..2000).map(|i| format!("{}\n", now - 2000 + i)).collect();
    let next = ledger_with_start(&existing, now);
    assert_eq!(next.lines().count(), LEDGER_MAX_ENTRIES);
    assert_eq!(next.lines().last(), Some("100000"));
}

#[test]
fn tonight_restart_cadence_is_a_loop() {
    // 2026-11-13: an OOM kill every ~4 minutes.
    let now = 50_000;
    let text: String = (0..15).map(|i| format!("{}\n", now - i * 240)).collect();
    assert!(starts_within(&text, now, RESTART_WINDOW_SECS) >= RESTART_LOOP_STARTS);
    // A deploy (two restarts) is not.
    assert!(starts_within(&format!("{}\n{}\n", now - 30, now), now, RESTART_WINDOW_SECS) < RESTART_LOOP_STARTS);
}

#[test]
fn memory_limits_inverted_by_a_runtime_drop_in_are_reported() {
    // Verbatim from the live unit on 2026-11-13.
    let finding = memory_limit_finding(
        "1610612736",
        "1468006400",
        "/run/systemd/system/dracon-sync-operator.service.d/10-resource-headroom.conf /run/systemd/system/dracon-sync-operator.service.d/30-tasksmax.conf",
    )
    .unwrap();
    assert!(finding.contains("MemoryHigh 1536M >= MemoryMax 1400M"), "{finding}");
    assert!(finding.contains("10-resource-headroom.conf"), "{finding}");
}

#[test]
fn coherent_or_unlimited_memory_limits_are_fine() {
    assert_eq!(memory_limit_finding("1073741824", "1468006400", ""), None);
    assert_eq!(memory_limit_finding("infinity", "1468006400", ""), None);
    assert_eq!(memory_limit_finding("1610612736", "infinity", ""), None);
    assert_eq!(memory_limit_finding("", "", ""), None);
}
