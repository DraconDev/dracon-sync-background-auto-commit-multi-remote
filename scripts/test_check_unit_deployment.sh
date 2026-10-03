#!/usr/bin/env bash
# Regression tests for check-unit-deployment.sh's drift detection.
#
# Mirrors dracon-system/scripts/test_check_unit_deployment.sh (same case
# numbering where the behaviour is shared; sync-only cases for the
# MemoryDenyWriteExecute sentinel, the %h storage roots, and the companion
# watchdog units).
#
# Every case drives the script with explicit repo/deployed paths so the host's
# real unit is never touched, and stubs the systemd-facing commands so the
# loaded-unit and verify steps are exercised deterministically instead of
# depending on whatever user manager the machine running the suite happens to
# have. Every negative case asserts a non-zero exit — the guard is worthless if
# it passes on drift.
set -euo pipefail

SCRIPT_UNDER_TEST="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/check-unit-deployment.sh"
# The unit name is fixed in the script under test (it is about this repo's
# unit); the file paths are what the suite varies.
UNIT_NAME="dracon-sync.service"
work=$(mktemp -d "${TMPDIR:-/tmp}/dracon-sync-unit-check-XXXXXX")
trap 'rm -rf "$work"' EXIT

# Run the whole suite against an empty HOME so unit discovery can only ever find
# a fixture. The header above promises the host's real unit is never touched,
# but a case that omits the second argument falls back to discovery — and on a
# host that has sync installed, the synthetic fixture never matches the real
# deployed unit, so the case failed at the byte-comparison step instead of
# testing what it was written to test. An empty HOME makes that class of leak a
# "nothing to compare" exit 0, which is the honest answer for a host that has
# not deployed anything.
export HOME="$work/home-isolated"
mkdir -p "$HOME"
unset XDG_CONFIG_HOME

fail() {
    echo "FAIL: $1" >&2
    exit 1
}

# --- systemd stubs -----------------------------------------------------------
# $1: what the loaded unit reports for -p MemoryDenyWriteExecute ("mdwe-yes"
# for a manager still loaded with the JIT-killer, anything else for clean, or
# "unreachable" for a host with no user manager at all).
make_systemctl() {
    local mode="$1" stub="$work/bin/systemctl-$1"
    mkdir -p "$work/bin"
    cat > "$stub" <<EOF
#!/usr/bin/env bash
# Fixture for mode: $mode
for arg in "\$@"; do
    case "\$arg" in
        -p)
            next_is_property=1
            continue
            ;;
    esac
    if [ "\${next_is_property:-0}" = 1 ]; then
        next_is_property=0
        property="\$arg"
    fi
done
if [ "\${property:-}" = Version ]; then
    case "$mode" in
        unreachable) exit 1 ;;
        *) printf '%s\n' 258.7; exit 0 ;;
    esac
fi
if [ "\${property:-}" = MemoryDenyWriteExecute ]; then
    case "$mode" in
        unreachable) exit 1 ;;
        mdwe-yes) printf '%s\n' yes; exit 0 ;;
        *) printf '%s\n' no; exit 0 ;;
    esac
fi
# Any other query: answer as a reachable manager would.
case "$mode" in
    unreachable) exit 1 ;;
    *) exit 0 ;;
esac
EOF
    chmod +x "$stub"
    printf '%s' "$stub"
}

# systemd-analyze stubs. Every case that stubs SYSTEMCTL also pins one of these,
# so the suite never inherits the invoking shell's real systemd-analyze: on a
# host with no user manager the real one exits 1 ("Failed to initialize
# manager"), which would make a clean case fail for a reason that has nothing to
# do with the condition under test — and would make a case expecting failure
# pass for the wrong reason.
analyze_clean="$work/bin/systemd-analyze-clean"
analyze_rejects="$work/bin/systemd-analyze-rejects"
mkdir -p "$work/bin"
cat > "$analyze_clean" <<'EOF'
#!/usr/bin/env bash
# Fixture: the unit verifies clean.
exit 0
EOF
cat > "$analyze_rejects" <<'EOF'
#!/usr/bin/env bash
echo "fixture.service: Command /nonexistent is not executable: No such file or directory" >&2
exit 1
EOF
chmod +x "$analyze_clean" "$analyze_rejects"

# --- fixtures ----------------------------------------------------------------
repo="$work/repo.service"
deployed="$work/deployed.service"
cat > "$repo" <<'EOF'
[Unit]
Description=fixture
[Service]
Type=simple
ExecStart=/bin/sh -c 'sleep 1'
# No MemoryDenyWriteExecute: the shipped unit must not set it (it kills the
# daemon on its first regex JIT compile — 2026-10-02).
ReadWritePaths=%h/.dracon %h/Dev
EOF

# 1. Identical copies, and the loaded unit really is free of the JIT-killer:
#    in sync.
cp "$repo" "$deployed"
out="$(SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" ||
    fail "identical units reported stale: $out"

# 2. The files agree but systemd is still loaded WITH MemoryDenyWriteExecute —
#    the file was fixed and copied, but `daemon-reload` was forgotten, so the
#    live daemon still crashes on regex JIT. The drift is still live.
out="$(SYSTEMCTL="$(make_systemctl mdwe-yes)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" &&
    fail "a copied-but-not-reloaded unit was reported as in sync"
grep -q 'daemon-reload' <<<"$out" || fail "no redeploy hint: $out"
grep -q 'MemoryDenyWriteExecute' <<<"$out" || fail "the divergent directive is not named: $out"

# 3. A host with no reachable user manager (CI, container, bare ssh) has no
#    opinion about the loaded unit. "Cannot ask systemd" must never be reported
#    as "systemd says no" — that is a false alarm with a wrong remediation.
out="$(SYSTEMCTL="$(make_systemctl unreachable)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" ||
    fail "an unreachable user manager was reported as stale: $out"
grep -q 'no reachable user systemd' <<<"$out" ||
    fail "the skipped-checks note is missing: $out"

# 4. Same case against the real commands with the bus env removed, which is how
#    a container reaches the script. Must take the documented exit-0 path.
out="$(env -u DBUS_SESSION_BUS_ADDRESS -u XDG_RUNTIME_DIR \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" ||
    fail "no-bus host was reported as stale: $out"

# 5. A repo copy that (re)introduces MemoryDenyWriteExecute=true is the
#    2026-10-02 bug in the file itself: the JIT-killer is shipped again.
printf 'MemoryDenyWriteExecute=true\n' >> "$repo"
out="$(SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" &&
    fail "a shipped unit setting MemoryDenyWriteExecute=true was reported as in sync"
grep -q 'MemoryDenyWriteExecute' <<<"$out" || fail "the JIT-killer is not named: $out"
grep -v '^MemoryDenyWriteExecute=' "$repo" > "$repo.fixed" && mv "$repo.fixed" "$repo"

# 6. A host that never deployed the unit is not drift, and must not fail — the
#    release pipeline runs this on machines without a user systemd.
if out="$("$SCRIPT_UNDER_TEST" "$repo" "$work/absent.service" 2>&1)"; then
    :
else
    fail "a missing deployed unit was treated as stale: $out"
fi
grep -q 'nothing to compare' <<<"$out" || fail "missing-unit output is not self-explanatory"

# 7. A missing repo copy is a script error, not a silent pass.
if "$SCRIPT_UNDER_TEST" "$work/absent.service" "$deployed" >/dev/null 2>&1; then
    fail "a missing repo unit copy was reported as clean"
fi

# 8. Comment-only differences still count as divergence: the deployed file must
#    be the shipped file, byte for byte, so an operator's local edit cannot
#    quietly shadow a shipped directive.
cp "$repo" "$deployed"
printf '# local operator note\n' >> "$deployed"
if SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" >/dev/null 2>&1; then
    fail "a locally edited deployed unit was reported as in sync"
fi

# 9. systemd-analyze rejecting the deployed file is a real failure, but only
#    when there is a manager to ask. The deployed copy is reset to the shipped
#    bytes: this case is about the verify step, so it must not inherit case 8's
#    mutation (which fails at the byte comparison instead) nor depend on what
#    unit discovery finds under the caller's HOME.
cp "$repo" "$deployed"
out="$(SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_rejects" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" &&
    fail "a unit rejected by systemd-analyze was reported as clean"
grep -q 'rejected' <<<"$out" || fail "the verify failure is not named: $out"

# 10. The same rejecting analyzer must NOT fail the run when no manager exists:
#     `systemd-analyze --user verify` cannot initialise without the bus and exits
#     non-zero with "Failed to initialize manager", which is not a verdict.
out="$(SYSTEMCTL="$(make_systemctl unreachable)" SYSTEMD_ANALYZE="$analyze_rejects" \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" ||
    fail "an unreachable manager made a reachable-verdict check fail: $out"

# 11. Hermeticity guard for this suite itself: a stubbed run must reach the same
#     verdict with the bus env stripped as with it present. This is the case that
#     catches a stubbed SYSTEMCTL paired with the real systemd-analyze, which
#     passes only on a host that happens to have a user manager.
out="$(env -u DBUS_SESSION_BUS_ADDRESS -u XDG_RUNTIME_DIR \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" ||
    fail "a stubbed run without a bus disagreed with a stubbed run with one: $out"

# 12. No HOME and no XDG_CONFIG_HOME either: `set -u` must not turn an unset
#     HOME into a hard failure. With no second argument there is no user unit
#     directory to compare, which is the same exit-0 "nothing to compare" path
#     as case 6.
out="$(env -u HOME -u XDG_CONFIG_HOME -u XDG_RUNTIME_DIR -u DBUS_SESSION_BUS_ADDRESS \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" ||
    fail "an unset HOME was treated as drift: $out"
grep -q 'no user unit directory to compare' <<<"$out" ||
    fail "the unset-HOME note is missing: $out"

# 13. Unit discovery must follow systemd, which searches BOTH
#     "$HOME/.config/systemd/user" and "$XDG_CONFIG_HOME/systemd/user". Checking
#     only one of them is a silent false pass: a drifted, deployed unit hides
#     behind the path that was not checked. In sync first...
fake_home="$work/home-fixture"
mkdir -p "$fake_home/.config/systemd/user"
cp "$repo" "$fake_home/.config/systemd/user/$UNIT_NAME"
out="$(env -u XDG_CONFIG_HOME HOME="$fake_home" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" ||
    fail "a unit deployed under HOME/.config was not found: $out"
grep -q "$fake_home/.config/systemd/user/$UNIT_NAME" <<<"$out" ||
    fail "the unit under HOME/.config was not the one compared: $out"
# ...then drifted, so discovery is proven to lead to a real verdict.
printf '# drifted\n' >> "$fake_home/.config/systemd/user/$UNIT_NAME"
if out="$(env -u XDG_CONFIG_HOME HOME="$fake_home" \
        SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
        "$SCRIPT_UNDER_TEST" "$repo" 2>&1)"; then
    fail "a drifted unit under HOME/.config was reported as in sync: $out"
fi
grep -q 'STALE' <<<"$out" || fail "drift under HOME/.config was not reported: $out"

# 14. The same for the XDG_CONFIG_HOME location, with a HOME that holds nothing.
xdg_home="$work/xdg-fixture"
empty_home="$work/home-empty"
mkdir -p "$xdg_home/systemd/user" "$empty_home"
cp "$repo" "$xdg_home/systemd/user/$UNIT_NAME"
out="$(env HOME="$empty_home" XDG_CONFIG_HOME="$xdg_home" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" ||
    fail "a unit deployed under XDG_CONFIG_HOME was not found: $out"
printf '# drifted\n' >> "$xdg_home/systemd/user/$UNIT_NAME"
if out="$(env HOME="$empty_home" XDG_CONFIG_HOME="$xdg_home" \
        SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
        "$SCRIPT_UNDER_TEST" "$repo" 2>&1)"; then
    fail "a drifted unit under XDG_CONFIG_HOME was reported as in sync: $out"
fi
grep -q 'STALE' <<<"$out" || fail "drift under XDG_CONFIG_HOME was not reported: $out"

# 15. A DANGLING symlink at the deployed path is still "deployed": `-f` follows
#     symlinks, so a unit linked to a GC'd nix store path tested false and the
#     script reported "nothing to compare" — a silent false pass over a broken
#     deployment (audit 2026-10-01).
dangling_home="$work/home-dangling"
mkdir -p "$dangling_home/.config/systemd/user"
ln -s "$xdg_home/removed-by-gc" "$dangling_home/.config/systemd/user/$UNIT_NAME"
out="$(env -u XDG_CONFIG_HOME HOME="$dangling_home" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" \
    && fail "a dangling unit symlink was reported as in sync: $out"
case "$out" in
    *"nothing to compare"*)
        fail "a dangling symlink must not be treated as 'not deployed': $out" ;;
    *"STALE"*|*"differs"*) : ;;
    *) fail "unexpected verdict for a dangling symlink: $out" ;;
esac

# 16. A REMOVED target of an otherwise-identical unit is likewise reported, not
#     silently accepted.
good_home="$work/home-good"
mkdir -p "$good_home/.config/systemd/user"
cp "$repo" "$good_home/.config/systemd/user/$UNIT_NAME"
out="$(env -u XDG_CONFIG_HOME HOME="$good_home" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" 2>&1)" \
    || fail "an identical unit was reported as drifted: $out"

# --- companion watchdog units --------------------------------------------------
# Step 3b: the watchdog units travel with the main unit and drift the same
# silent way. A deployed companion that differs from the shipped file is STALE;
# a companion that was never deployed is a note, not a verdict.
shipped_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
companion_repo="$shipped_dir/dracon-sync-watchdog.service"
[ -f "$companion_repo" ] || fail "shipped companion $companion_repo is missing (audit M8 ships it)"

# 17. A deployed companion identical to the shipped file passes.
cp "$repo" "$deployed"
cp "$companion_repo" "$work/dracon-sync-watchdog.service"
out="$(SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" ||
    fail "an identical companion unit was reported as drifted: $out"

# 18. A deployed companion that differs from the shipped file is STALE.
printf '# drifted\n' >> "$work/dracon-sync-watchdog.service"
out="$(SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" &&
    fail "a drifted companion unit was reported as in sync: $out"
grep -q 'STALE' <<<"$out" || fail "companion drift was not reported: $out"
rm -f "$work/dracon-sync-watchdog.service"

# 19. A companion that was never deployed is a note, never a failure — fresh
#     installs gain the backstops through install.sh and the flake module.
out="$(SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$deployed" 2>&1)" ||
    fail "a never-deployed companion was treated as drift: $out"
grep -q 'shipped but not deployed' <<<"$out" ||
    fail "the never-deployed companion note is missing: $out"

# --- runtime storage-root check ----------------------------------------------
# Steps 1-5 compare files; none of them can see that a ReadWritePaths entry
# which is correct on disk still granted nothing at runtime. That is the
# guard's 2026-09-28..10-01 breakage; sync's equivalent is a daemon that can
# read every repo but cannot write its state under ~/.dracon, its repos under
# ~/Dev, or its logs under ~/.local/state/dracon. These cases pin the runtime
# contract against mountinfo fixtures, so no host with a live service is
# required. Roots come from the unit's own ReadWritePaths line, %h expanded
# against HOME.

# 20. The broken shape: the disk is read-only and no subtree has its own mount,
#     so every state write fails. MUST be a failure, not a pass.
repo_abs="$work/repo-abs.service"
cat > "$repo_abs" <<'EOF'
[Unit]
Description=fixture
[Service]
Type=simple
ExecStart=/bin/sh -c 'sleep 1'
ReadWritePaths=/mnt/data/state /mnt/data/repos
EOF
ro_disk="$work/mountinfo-ro-disk"
cat <<'EOF' > "$ro_disk"
622 240 259:2 / / ro,nosuid,relatime shared:252 master:1 - ext4 /dev/nvme0n1p2 rw
650 622 8:2 / /mnt/data ro,nosuid,noatime shared:527 master:129 - ext4 /dev/sda2 rw
EOF
out="$(SYNC_MOUNTINFO="$ro_disk" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo_abs" "$repo_abs" 2>&1)" \
    && fail "a read-only storage root was reported as in sync: $out"
case "$out" in
    *"resolves read-only"*)
        case "$out" in
            *"/mnt/data/state"*) : ;;
            *) fail "the read-only report did not name /mnt/data/state: $out" ;;
        esac
        ;;
    *) fail "unexpected verdict for a read-only storage root: $out" ;;
esac

# 21. Granting the disk but not the subtree is still broken: the entry has to
#     name the path itself, because ProtectSystem=strict makes the parent
#     read-only no matter what the disk itself reports.
partial="$work/mountinfo-partial"
cat <<'EOF' > "$partial"
622 240 259:2 / / ro,nosuid,relatime shared:252 master:1 - ext4 /dev/nvme0n1p2 rw
650 622 8:2 / /mnt/data ro,nosuid,noatime shared:527 master:129 - ext4 /dev/sda2 rw
675 650 8:2 /repos /mnt/data/repos rw,nosuid,noatime shared:528 master:129 - ext4 /dev/sda2 rw
EOF
out="$(SYNC_MOUNTINFO="$partial" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo_abs" "$repo_abs" 2>&1)" \
    && fail "a grant on the disk but not on the subtree was accepted: $out"
case "$out" in
    *"/mnt/data/state resolves read-only"*) : ;;
    *) fail "unexpected verdict for a partially granted subtree: $out" ;;
esac
case "$out" in
    *"/mnt/data/repos is read-write"*) : ;;
    *) fail "the granted root should have passed: $out" ;;
esac

# 22. The fixed shape passes and names the mounts it relied on.
fixed="$work/mountinfo-fixed"
cat <<'EOF' > "$fixed"
622 240 259:2 / / ro,nosuid,relatime shared:252 master:1 - ext4 /dev/nvme0n1p2 rw
650 622 8:2 / /mnt/data ro,nosuid,noatime shared:527 master:129 - ext4 /dev/sda2 rw
675 650 8:2 /repos /mnt/data/repos rw,nosuid,noatime shared:528 master:129 - ext4 /dev/sda2 rw
676 650 8:2 /state /mnt/data/state rw,nosuid,noatime shared:529 master:129 - ext4 /dev/sda2 rw
EOF
out="$(SYNC_MOUNTINFO="$fixed" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo_abs" "$repo_abs" 2>&1)" \
    || fail "the fixed namespace was reported as broken: $out"
case "$out" in
    *"✓ ReadWritePaths root /mnt/data/state is read-write"*) : ;;
    *) fail "no pass line for /mnt/data/state: $out" ;;
esac

# 23. %h roots expand against HOME and resolve like any other root.
home_mount="$work/mountinfo-home"
cat <<EOF > "$home_mount"
622 240 259:2 / / ro,nosuid,relatime shared:252 master:1 - ext4 /dev/nvme0n1p2 rw
700 622 259:2 $HOME $HOME rw,nosuid,relatime shared:900 master:1 - ext4 /dev/nvme0n1p2 rw
EOF
out="$(SYNC_MOUNTINFO="$home_mount" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$repo" 2>&1)" \
    || fail "a read-write HOME subtree was rejected: $out"
case "$out" in
    *"$HOME/.dracon is read-write"*) : ;;
    *) fail "no pass line for the %h-expanded root: $out" ;;
esac

# 24. A later mount at the same point shadows an earlier read-write one, which
#     is how ProtectHome/ProtectSystem end up making paths read-only. Reading
#     the first match instead of the effective one would call a shadowed root
#     writable when it is not.
shadow="$work/mountinfo-shadowed"
cat <<EOF > "$shadow"
700 622 259:2 $HOME $HOME rw,nosuid,relatime shared:900 master:1 - ext4 /dev/nvme0n1p2 rw
1018 700 259:2 $HOME $HOME ro,nosuid,relatime shared:901 master:1 - ext4 /dev/nvme0n1p2 rw
EOF
out="$(SYNC_MOUNTINFO="$shadow" \
    SYSTEMCTL="$(make_systemctl present)" SYSTEMD_ANALYZE="$analyze_clean" \
    "$SCRIPT_UNDER_TEST" "$repo" "$repo" 2>&1)" \
    && fail "a shadowed read-only mount was reported writable: $out"
case "$out" in
    *"resolves read-only"*) : ;;
    *) fail "the shadowed mount was not detected: $out" ;;
esac

echo "✓ all check-unit-deployment regression cases passed"
