//! Workspace-local release and repository tooling.

mod audit;
mod package;
mod release;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Debug, Parser)]
#[command(name = "cargo xtask", version, about)]
struct Cli {
    /// Public workspace root. Defaults to the current Cargo workspace.
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Validate metadata, repository policy, workflows, links, and assets.
    Audit {
        /// Run fail-closed positive and negative fixtures only.
        #[arg(long)]
        self_test: bool,

        /// Validate an additional directory containing workflow YAML files.
        #[arg(long)]
        extra_workflow_root: Vec<PathBuf>,

        /// Validate subjects in this Git revision range.
        #[arg(long)]
        commit_range: Option<String>,

        /// Validate an explicit future commit subject.
        #[arg(long)]
        commit_subject: Vec<String>,

        /// Validate only commit subjects, not the repository candidate.
        #[arg(long)]
        commit_subjects_only: bool,

        /// Emit one machine-readable JSON object.
        #[arg(long)]
        json: bool,
    },

    /// Build a full preflight or selected publish-equivalent crate archives.
    PackageCheck {
        /// Inspect package file lists without producing `.crate` archives.
        #[arg(long)]
        list_only: bool,

        /// Permit an uncommitted worktree during local preparation.
        #[arg(long)]
        allow_dirty: bool,

        /// Build selected publish-equivalent archives and update their ledger.
        #[arg(long = "package")]
        packages: Vec<String>,
    },

    /// Validate that the current workspace revision is ready for release.
    ReleaseCheck {
        /// Expected lifecycle state of the candidate.
        #[arg(long, value_enum, default_value_t = ReleaseState::Candidate)]
        state: ReleaseState,

        /// Permit an uncommitted worktree during local preparation.
        #[arg(long)]
        allow_dirty: bool,

        /// Skip archive generation when it was already proved separately.
        #[arg(long)]
        skip_package_check: bool,

        /// Assert the triggering tag name against the synchronized workspace version.
        #[arg(long)]
        tag: Option<String>,
    },

    /// Print the dependency-derived topological publication plan.
    ReleasePlan {
        /// Emit the plan as JSON.
        #[arg(long)]
        json: bool,
    },

    /// Print only the current release section as reusable Markdown notes.
    ReleaseNotes {
        /// Version to render. Defaults to the synchronized workspace version.
        #[arg(long)]
        version: Option<String>,
    },

    /// Verify that every framework crate and version exists on crates.io.
    VerifyRelease {
        /// Version to verify. Defaults to the synchronized workspace version.
        #[arg(long)]
        version: Option<String>,

        /// Limit verification to selected package names; may be repeated.
        #[arg(long = "package")]
        packages: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, ValueEnum)]
enum ReleaseState {
    /// Prepared locally, before the release commit is selected.
    #[default]
    Candidate,
    /// Clean, reviewed, and taggable, but not tagged.
    ReleaseReady,
    /// The exact release tag points at `HEAD`.
    Tagged,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = audit::resolve_workspace_root(cli.root.as_deref())?;

    match cli.command {
        Command::Audit {
            self_test,
            extra_workflow_root,
            commit_range,
            commit_subject,
            commit_subjects_only,
            json,
        } => audit::command(
            &root,
            audit::AuditOptions {
                self_test,
                extra_workflow_roots: extra_workflow_root,
                commit_range,
                commit_subjects: commit_subject,
                commit_subjects_only,
                json,
            },
        ),
        Command::PackageCheck {
            list_only,
            allow_dirty,
            packages,
        } => package::command(&root, list_only, allow_dirty, &packages),
        Command::ReleaseCheck {
            state,
            allow_dirty,
            skip_package_check,
            tag,
        } => release::check(
            &root,
            state,
            allow_dirty,
            skip_package_check,
            tag.as_deref(),
        ),
        Command::ReleasePlan { json } => release::plan_command(&root, json),
        Command::ReleaseNotes { version } => release::notes(&root, version.as_deref()),
        Command::VerifyRelease { version, packages } => {
            release::verify(&root, version.as_deref(), &packages)
        }
    }
}
