//! Private security-output publication. Unapproved spools are never upload inputs.

use super::*;

pub(crate) struct SecuritySpool {
    pub(crate) file: File,
    pub(crate) capacity: u64,
    _budget: File,
}

impl JobLease {
    pub(crate) fn security_spool(&self) -> Result<SecuritySpool> {
        let job = self.load()?;
        job.require_phase(&[Phase::Captured])?;
        if job.spec.encryption != Encryption::WardenAge || job.prepared_candidate.is_some() {
            bail!("fresh security processing requires an unselected encrypted job");
        }
        // Recreating an unapproved transform cannot lose the captured source.
        self.source_snapshot()?;
        let budget = try_lock(&self.directory.join("payload-budget.lock"))?;
        let path = self.directory.join(format!("{}.security-output", self.id));
        let (retained, previous) = snapshot_bytes(
            &self.directory,
            &path,
            &["payload", "payload-capture", "security-output"],
        )?;
        let capacity = self.limits.max_retained_payload_bytes
            .checked_sub(retained.checked_sub(previous).context("invalid spool accounting")?)
            .context("retained payload budget exhausted")?
            .min(self.limits.max_payload_bytes);
        if capacity == 0 { bail!("retained payload budget exhausted"); }
        let file = open_private(&path, true, true)?;
        // No candidate/approval exists: this artifact was an interrupted/failed
        // transform, never an approved version. Only this job's spool is reset.
        file.set_len(0)?;
        Ok(SecuritySpool { file, capacity, _budget: budget })
    }

    pub(crate) fn approve_security_output(&self, spool: SecuritySpool, identity: Fingerprint) -> Result<Job> {
        if identity.bytes() > spool.capacity { bail!("security output exceeds reserved capacity"); }
        spool.file.sync_all()?;
        let path = self.directory.join(format!("{}.security-output", self.id));
        verify_snapshot(&path, &identity)?;
        let mut job = self.load()?;
        job.select_prepared_payload(identity)?;
        crash_point("before-security-approval");
        self.save(&mut job)?;
        crash_point("after-security-approval");
        // Retain the budget lease across both approval and publication.
        self.finish_security_output(job)
    }

    pub(crate) fn recover_security_output(&self) -> Result<Option<Job>> {
        let job = self.load()?;
        if job.phase == Phase::Prepared {
            self.payload_snapshot()?;
            return Ok(Some(job));
        }
        job.require_phase(&[Phase::Captured])?;
        if job.prepared_candidate.is_none() { return Ok(None); }
        let _budget = try_lock(&self.directory.join("payload-budget.lock"))?;
        Ok(Some(self.finish_security_output(job)?))
    }

    fn finish_security_output(&self, mut job: Job) -> Result<Job> {
        let expected = job.prepared_candidate.clone().context("security approval missing")?;
        let destination = self.directory.join(format!("{}.payload", self.id));
        if exists_without_symlink(&destination)? {
            verify_snapshot(&destination, &expected)?.sync_all()?;
        } else {
            let path = self.directory.join(format!("{}.security-output", self.id));
            verify_snapshot(&path, &expected)?.sync_all()?;
            std::fs::rename(path, &destination)?;
        }
        File::open(&self.directory)?.sync_all()?;
        crash_point("after-security-publish");
        job.record_prepared(expected)?;
        self.save(&mut job)?;
        Ok(job)
    }
}
