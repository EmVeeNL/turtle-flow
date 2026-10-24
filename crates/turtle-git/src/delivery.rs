//! Local delivery without touching the user's checkout (`docs/GIT-STRATEGY.md` §5).
//!
//! The squash commit is built with plumbing (`merge-tree`, `commit-tree`) and the target ref is
//! moved with compare-and-swap `update-ref`. A branch that is checked out in any worktree is
//! never moved: the result is parked at `refs/turtle/delivery/<job>` and the job becomes
//! "pending integration".

use crate::merge::MergeProbe;
use crate::{GitError, Repo, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// The target branch now points at `commit`.
    Delivered { commit: String },
    /// The target is checked out somewhere; `commit` is parked at `reference` for the user to
    /// fast-forward (`git merge --ff-only <reference>`).
    PendingIntegration { commit: String, reference: String },
}

#[derive(Debug, Clone)]
pub struct SquashRequest<'a> {
    pub target_branch: &'a str,
    pub job_branch: &'a str,
    pub job_key: &'a str,
    pub subject: &'a str,
    pub body: &'a str,
    pub trailers: &'a str,
}

impl Repo {
    /// Squash-delivers `job_branch` onto `target_branch`.
    pub async fn deliver_squash(&self, req: &SquashRequest<'_>) -> Result<DeliveryOutcome> {
        let tip = self.branch_tip(req.target_branch).await?.ok_or_else(|| {
            GitError::Invalid(format!(
                "target branch `{}` does not exist",
                req.target_branch
            ))
        })?;
        self.deliver_squash_from(req, &tip).await
    }

    /// Like [`Repo::deliver_squash`], but built on `target_tip`, the tip that was validated and
    /// approved. The ref is only moved if the target is still exactly there, so a target that
    /// moved in the meantime fails with [`GitError::TargetMoved`] instead of being overwritten.
    pub async fn deliver_squash_from(
        &self,
        req: &SquashRequest<'_>,
        target_tip: &str,
    ) -> Result<DeliveryOutcome> {
        if self
            .run(&[
                "check-ref-format",
                &format!("refs/heads/{}", req.target_branch),
            ])
            .await
            .is_err()
        {
            return Err(GitError::Invalid(format!(
                "invalid target branch `{}`",
                req.target_branch
            )));
        }
        let target_ref = format!("refs/heads/{}", req.target_branch);
        let target_tip = self.rev(target_tip).await?;
        let head = self.rev(req.job_branch).await?;

        let tree = match self.probe_merge(&target_tip, &head).await? {
            MergeProbe::Clean { tree } => tree,
            MergeProbe::Conflicts { files } => return Err(GitError::Conflicts(files)),
        };

        let mut message = req.subject.to_owned();
        if !req.body.trim().is_empty() {
            message.push_str("\n\n");
            message.push_str(req.body.trim_end());
        }
        message.push_str("\n\n");
        message.push_str(req.trailers);
        message.push('\n');
        let commit = self
            .run_stdin(
                &["commit-tree", &tree, "-p", &target_tip, "-F", "-"],
                &message,
            )
            .await?;

        // Never move a branch that is checked out: that would desynchronize someone's work tree.
        if self
            .checked_out_branches()
            .await?
            .iter()
            .any(|b| b == req.target_branch)
        {
            let reference = format!("refs/turtle/delivery/{}", req.job_key);
            self.run(&["update-ref", &reference, &commit]).await?;
            return Ok(DeliveryOutcome::PendingIntegration { commit, reference });
        }

        // Compare-and-swap: fails if the target moved since `target_tip` was read.
        self.run(&["update-ref", &target_ref, &commit, &target_tip])
            .await
            .map_err(|_| GitError::TargetMoved {
                branch: req.target_branch.to_owned(),
            })?;
        Ok(DeliveryOutcome::Delivered { commit })
    }

    /// Recovery probe: did a delivery for this job already land on the target?
    pub async fn find_delivery_commit(
        &self,
        target_branch: &str,
        job_key: &str,
    ) -> Result<Option<String>> {
        let pattern = format!("^Turtle-Job: {job_key}$");
        let out = self
            .run(&[
                "log",
                "-n",
                "1",
                "--format=%H",
                "-E",
                "--grep",
                &pattern,
                target_branch,
            ])
            .await?;
        Ok((!out.is_empty()).then_some(out))
    }
}
