//! Non-mutating standard-LFS routing decisions.
//!
//! This evaluator does not classify files, enroll paths, configure filters, or
//! authorize uploads. Its evidence must come from separately reviewed adapters.

/// Reviewed content classification; extensions alone cannot establish this.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Classification {
    /// Explicitly approved, non-sensitive asset snapshot.
    ApprovedAsset,
    /// Ordinary source text, including SQL migrations.
    SourceText,
    /// Sensitive content owned by Warden.
    Sensitive,
    /// Database, dump, WAL, or SHM content.
    Database,
    /// Unknown or unreviewed payload (including opaque archives).
    Unknown,
}

/// Effective Git filter after all attribute precedence is resolved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    /// Ordinary Git representation.
    Git,
    /// Verified stock LFS driver.
    Lfs,
    /// Warden or another conflicting driver.
    Conflicting,
}

/// Explicit tuning, not deployment defaults. No policy means no enrollment.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Global enablement.
    pub enabled: bool,
    /// Minimum size under repository pressure or churn.
    pub minimum_asset_bytes: u64,
    /// Individual-file trigger independent of repository measurement.
    pub individual_trigger_bytes: u64,
    /// Own-repository history pressure threshold (not nested totals).
    pub history_trigger_bytes: u64,
    /// Reviewed candidate churn threshold.
    pub churn_trigger_bytes: u64,
}

/// Explicit per-repository tuning. Unspecified values inherit global tuning.
/// This type is not yet wired into the daemon's TOML schema.
#[derive(Clone, Copy, Debug, Default)]
pub struct PolicyOverride {
    /// Override enablement (including opt-out).
    pub enabled: Option<bool>,
    /// Override minimum eligible size.
    pub minimum_asset_bytes: Option<u64>,
    /// Override individual-file trigger.
    pub individual_trigger_bytes: Option<u64>,
    /// Override repository pressure threshold.
    pub history_trigger_bytes: Option<u64>,
    /// Override candidate churn threshold.
    pub churn_trigger_bytes: Option<u64>,
}

impl Policy {
    /// Resolve per-repository tuning without changing the global policy.
    pub fn with_override(self, overrides: PolicyOverride) -> Self {
        Self {
            enabled: overrides.enabled.unwrap_or(self.enabled),
            minimum_asset_bytes: overrides
                .minimum_asset_bytes
                .unwrap_or(self.minimum_asset_bytes),
            individual_trigger_bytes: overrides
                .individual_trigger_bytes
                .unwrap_or(self.individual_trigger_bytes),
            history_trigger_bytes: overrides
                .history_trigger_bytes
                .unwrap_or(self.history_trigger_bytes),
            churn_trigger_bytes: overrides
                .churn_trigger_bytes
                .unwrap_or(self.churn_trigger_bytes),
        }
    }

    /// Reject zero thresholds and an individual trigger below the minimum size.
    pub fn is_valid(self) -> bool {
        self.minimum_asset_bytes > 0
            && self.individual_trigger_bytes >= self.minimum_asset_bytes
            && self.history_trigger_bytes > 0
            && self.churn_trigger_bytes > 0
    }
}

/// Evidence for one stable snapshot. Unknown measurements stay unknown.
#[derive(Clone, Copy, Debug)]
pub struct Evidence {
    /// Classification tied to this snapshot by the caller.
    pub classification: Classification,
    /// Effective filter.
    pub filter: Filter,
    /// Stable worktree size.
    pub bytes: u64,
    /// Measured own-repository history bytes.
    pub history_bytes: Option<u64>,
    /// Measured churn bytes for the candidate.
    pub churn_bytes: Option<u64>,
    /// Previously enrolled path; must survive size reductions.
    pub enrolled: bool,
    /// Operator-approved private binding, not an arbitrary repo URL.
    pub approved_binding: bool,
    /// Explicit per-repository opt-out.
    pub opted_out: bool,
    /// Ignore/generated-output rules take precedence.
    pub excluded: bool,
}

/// Reasons requiring preservation of existing state, never raw-byte fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HoldReason {
    /// Tuning is invalid; do not silently enroll every asset.
    InvalidPolicy,
    /// The current payload is not an approved asset.
    Classification,
    /// Effective attributes conflict with the selected representation.
    FilterConflict,
    /// An enrolled path lost required enablement or private binding.
    EnrollmentUnavailable,
}

/// Representation decision only; affirmative decisions do not authorize upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Existing exclusions remain effective; do not stage.
    Excluded,
    /// Use ordinary Git subject to unchanged staging/security limits.
    KeepGit,
    /// Preserve verified sticky LFS representation.
    RetainLfs,
    /// Candidate for transactional enrollment after further security checks.
    EnrollLfs,
    /// Preserve state and report concern; never silently revert to raw Git.
    Hold(HoldReason),
}

/// Evaluate evidence without filesystem, network, or configuration mutations.
pub fn evaluate(policy: Option<Policy>, evidence: Evidence) -> Decision {
    if evidence.excluded {
        return Decision::Excluded;
    }
    let sticky = evidence.enrolled || evidence.filter == Filter::Lfs;
    if evidence.classification != Classification::ApprovedAsset {
        return if sticky || evidence.classification != Classification::SourceText {
            Decision::Hold(HoldReason::Classification)
        } else {
            Decision::KeepGit
        };
    }
    if evidence.filter == Filter::Conflicting
        || (evidence.enrolled && evidence.filter != Filter::Lfs)
    {
        return Decision::Hold(HoldReason::FilterConflict);
    }
    let active = policy.filter(|p| p.enabled && !evidence.opted_out && evidence.approved_binding);
    let Some(policy) = active else {
        return if sticky {
            Decision::Hold(HoldReason::EnrollmentUnavailable)
        } else {
            Decision::KeepGit
        };
    };
    if !policy.is_valid() {
        return Decision::Hold(HoldReason::InvalidPolicy);
    }
    if sticky {
        return Decision::RetainLfs;
    }
    let pressure = evidence
        .history_bytes
        .is_some_and(|n| n >= policy.history_trigger_bytes);
    let churn = evidence
        .churn_bytes
        .is_some_and(|n| n >= policy.churn_trigger_bytes);
    if evidence.bytes >= policy.individual_trigger_bytes
        || (evidence.bytes >= policy.minimum_asset_bytes && (pressure || churn))
    {
        Decision::EnrollLfs
    } else {
        Decision::KeepGit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const MIB: u64 = 1024 * 1024;
    fn policy() -> Policy {
        Policy {
            enabled: true,
            minimum_asset_bytes: 20 * MIB,
            individual_trigger_bytes: 90 * MIB,
            history_trigger_bytes: 1024 * MIB,
            churn_trigger_bytes: 100 * MIB,
        }
    }
    fn asset() -> Evidence {
        Evidence {
            classification: Classification::ApprovedAsset,
            filter: Filter::Git,
            bytes: 10 * MIB,
            history_bytes: Some(200 * MIB),
            churn_bytes: Some(0),
            enrolled: false,
            approved_binding: true,
            opted_out: false,
            excluded: false,
        }
    }
    #[test]
    fn small_image_repository_stays_git() {
        assert_eq!(evaluate(Some(policy()), asset()), Decision::KeepGit);
    }
    #[test]
    fn pressure_and_churn_require_minimum_asset_size() {
        for churn in [false, true] {
            let mut e = asset();
            if churn {
                e.churn_bytes = Some(100 * MIB);
            } else {
                e.history_bytes = Some(1024 * MIB);
            }
            assert_eq!(evaluate(Some(policy()), e), Decision::KeepGit);
            e.bytes = 20 * MIB;
            assert_eq!(evaluate(Some(policy()), e), Decision::EnrollLfs);
        }
    }
    #[test]
    fn unknown_history_does_not_mask_individual_trigger() {
        let mut e = asset();
        e.history_bytes = None;
        e.churn_bytes = None;
        assert_eq!(evaluate(Some(policy()), e), Decision::KeepGit);
        e.bytes = 101 * MIB;
        assert_eq!(evaluate(Some(policy()), e), Decision::EnrollLfs);
    }
    #[test]
    fn disabled_optout_and_unapproved_binding_prevent_enrollment() {
        let mut e = asset();
        e.bytes = 101 * MIB;
        assert_eq!(evaluate(None, e), Decision::KeepGit);
        let mut p = policy();
        p.enabled = false;
        assert_eq!(evaluate(Some(p), e), Decision::KeepGit);
        e.opted_out = true;
        assert_eq!(evaluate(Some(policy()), e), Decision::KeepGit);
        e.opted_out = false;
        e.approved_binding = false;
        assert_eq!(evaluate(Some(policy()), e), Decision::KeepGit);
    }
    #[test]
    fn sticky_paths_never_fall_back_to_raw_git() {
        let mut e = asset();
        e.enrolled = true;
        e.filter = Filter::Lfs;
        e.bytes = 1;
        assert_eq!(evaluate(Some(policy()), e), Decision::RetainLfs);
        e.opted_out = true;
        assert_eq!(
            evaluate(Some(policy()), e),
            Decision::Hold(HoldReason::EnrollmentUnavailable)
        );
        e.opted_out = false;
        e.filter = Filter::Git;
        assert_eq!(
            evaluate(Some(policy()), e),
            Decision::Hold(HoldReason::FilterConflict)
        );
    }
    #[test]
    fn every_effective_lfs_path_preserves_representation_on_failure() {
        // Include paths discovered through attributes without a persisted ledger.
        for enrolled in [false, true] {
            for classification in [
                Classification::ApprovedAsset,
                Classification::SourceText,
                Classification::Sensitive,
                Classification::Database,
                Classification::Unknown,
            ] {
                for approved_binding in [false, true] {
                    for opted_out in [false, true] {
                        let mut invalid = policy();
                        invalid.churn_trigger_bytes = 0;
                        let mut disabled = policy();
                        disabled.enabled = false;
                        for tuning in [None, Some(policy()), Some(invalid), Some(disabled)] {
                            let e = Evidence {
                                enrolled,
                                classification,
                                approved_binding,
                                opted_out,
                                filter: Filter::Lfs,
                                bytes: 1,
                                ..asset()
                            };
                            let expected = if classification != Classification::ApprovedAsset {
                                Decision::Hold(HoldReason::Classification)
                            } else if tuning.is_none_or(|p| !p.enabled)
                                || !approved_binding
                                || opted_out
                            {
                                Decision::Hold(HoldReason::EnrollmentUnavailable)
                            } else if !tuning.unwrap().is_valid() {
                                Decision::Hold(HoldReason::InvalidPolicy)
                            } else {
                                Decision::RetainLfs
                            };
                            assert_eq!(evaluate(tuning, e), expected);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn sensitive_database_and_unknown_payloads_are_held() {
        for classification in [
            Classification::Sensitive,
            Classification::Database,
            Classification::Unknown,
        ] {
            let mut e = asset();
            e.bytes = 101 * MIB;
            e.classification = classification;
            assert_eq!(
                evaluate(Some(policy()), e),
                Decision::Hold(HoldReason::Classification)
            );
        }
    }
    #[test]
    fn source_text_is_not_blanket_excluded() {
        let mut e = asset();
        e.classification = Classification::SourceText;
        assert_eq!(evaluate(Some(policy()), e), Decision::KeepGit);
        e.filter = Filter::Lfs;
        assert_eq!(
            evaluate(Some(policy()), e),
            Decision::Hold(HoldReason::Classification)
        );
    }
    #[test]
    fn overrides_inherit_and_change_each_threshold() {
        let p = policy();
        let inherited = p.with_override(PolicyOverride::default());
        assert_eq!(inherited.minimum_asset_bytes, p.minimum_asset_bytes);
        assert_eq!(
            inherited.individual_trigger_bytes,
            p.individual_trigger_bytes
        );
        assert_eq!(inherited.history_trigger_bytes, p.history_trigger_bytes);
        assert_eq!(inherited.churn_trigger_bytes, p.churn_trigger_bytes);
        let mut e = asset();
        let optout = p.with_override(PolicyOverride {
            enabled: Some(false),
            ..Default::default()
        });
        e.bytes = 101 * MIB;
        assert_eq!(evaluate(Some(optout), e), Decision::KeepGit);
        e = asset();
        let smaller = p.with_override(PolicyOverride {
            individual_trigger_bytes: Some(20 * MIB),
            ..Default::default()
        });
        e.bytes = 20 * MIB;
        assert_eq!(evaluate(Some(smaller), e), Decision::EnrollLfs);
        e = asset();
        let pressure = p.with_override(PolicyOverride {
            minimum_asset_bytes: Some(10 * MIB),
            history_trigger_bytes: Some(200 * MIB),
            ..Default::default()
        });
        assert_eq!(evaluate(Some(pressure), e), Decision::EnrollLfs);
        let churn = p.with_override(PolicyOverride {
            minimum_asset_bytes: Some(10 * MIB),
            churn_trigger_bytes: Some(5 * MIB),
            ..Default::default()
        });
        e.churn_bytes = Some(5 * MIB);
        assert_eq!(evaluate(Some(churn), e), Decision::EnrollLfs);
        assert_eq!(evaluate(Some(p), e), Decision::KeepGit);
    }
    #[test]
    fn invalid_thresholds_fail_closed() {
        for overrides in [
            PolicyOverride {
                minimum_asset_bytes: Some(0),
                ..Default::default()
            },
            PolicyOverride {
                individual_trigger_bytes: Some(1),
                ..Default::default()
            },
            PolicyOverride {
                history_trigger_bytes: Some(0),
                ..Default::default()
            },
            PolicyOverride {
                churn_trigger_bytes: Some(0),
                ..Default::default()
            },
        ] {
            assert_eq!(
                evaluate(Some(policy().with_override(overrides)), asset()),
                Decision::Hold(HoldReason::InvalidPolicy)
            );
        }
    }
    #[test]
    fn exclusions_and_conflicting_filters_win() {
        let mut e = asset();
        e.filter = Filter::Conflicting;
        assert_eq!(
            evaluate(Some(policy()), e),
            Decision::Hold(HoldReason::FilterConflict)
        );
        e.excluded = true;
        assert_eq!(evaluate(Some(policy()), e), Decision::Excluded);
    }
}
