use std::{
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use crate::agents::opencode::run_command;

pub fn git(directory: &Path, args: &[&str]) -> Result<String, String> {
    let mut command = Command::new("git");
    command.current_dir(directory).args(args);
    // Local automation identity without modifying the user's Git configuration.
    command
        .env("GIT_AUTHOR_NAME", "Software Factory")
        .env("GIT_AUTHOR_EMAIL", "sf@localhost")
        .env("GIT_COMMITTER_NAME", "Software Factory")
        .env("GIT_COMMITTER_EMAIL", "sf@localhost");
    let output = run_command(command, Duration::from_secs(120)).map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn prepare(directory: &Path) -> Result<(), String> {
    if !directory.join(".git").exists() {
        git(directory, &["init", "-b", "main"])?;
        git(directory, &["add", "--all"])?;
        git(
            directory,
            &["commit", "--allow-empty", "-m", "chore: initialize project"],
        )?;
    } else {
        if !git(directory, &["status", "--porcelain"])?.is_empty() {
            return Err(
                "project has uncommitted changes; commit or stash them before enabling execution"
                    .into(),
            );
        }
        if git(directory, &["rev-parse", "--verify", "HEAD"]).is_err() {
            return Err(
                "existing repository has no commit; initialize it before enabling execution".into(),
            );
        }
        if git(directory, &["show-ref", "--verify", "refs/heads/main"]).is_err() {
            git(directory, &["branch", "main", "HEAD"])?;
        }
        if git(directory, &["branch", "--show-current"])? != "main" {
            git(directory, &["switch", "main"])?;
        }
    }
    Ok(())
}

pub fn create_worktree(
    directory: &Path,
    task_id: &str,
) -> Result<(String, PathBuf, String), String> {
    prepare(directory)?;
    let base = git(directory, &["rev-parse", "main"])?;
    let branch = format!("sf/task-{task_id}");
    let parent = directory
        .parent()
        .ok_or("project directory has no parent")?
        .join(".sf-worktrees");
    std::fs::create_dir_all(&parent).map_err(|e| e.to_string())?;
    let worktree = parent.join(task_id);
    git(
        directory,
        &[
            "worktree",
            "add",
            "-b",
            &branch,
            &worktree.to_string_lossy(),
            &base,
        ],
    )?;
    Ok((branch, worktree, base))
}

pub fn verify(directory: &Path, commands: &[String]) -> Result<(), String> {
    for text in commands {
        let mut command = Command::new("sh");
        command.current_dir(directory).args(["-c", text]);
        let timeout = std::env::var("SF_AGENT_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(600);
        let output =
            run_command(command, Duration::from_secs(timeout)).map_err(|e| e.to_string())?;
        if !output.status.success() {
            return Err(format!(
                "verification `{text}` failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }
    Ok(())
}

pub fn incorporate_main(worktree: &Path) -> Result<(), String> {
    if let Err(error) = git(worktree, &["merge", "--no-edit", "main"]) {
        let _ = git(worktree, &["merge", "--abort"]);
        return Err(error);
    }
    Ok(())
}

pub fn merge(directory: &Path, branch: &str) -> Result<(), String> {
    if git(directory, &["branch", "--show-current"])? != "main"
        || !git(directory, &["status", "--porcelain"])?.is_empty()
    {
        return Err("main must be checked out and clean before integration".into());
    }
    git(directory, &["merge", "--ff-only", branch])?;
    Ok(())
}

pub fn cleanup(directory: &Path, worktree: &Path) -> Result<(), String> {
    git(
        directory,
        &["worktree", "remove", &worktree.to_string_lossy()],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_branches_integrate_from_main_and_remove_worktrees() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let (a, wa, base_a) = create_worktree(&project, "a").unwrap();
        let (b, wb, base_b) = create_worktree(&project, "b").unwrap();
        assert_eq!(base_a, base_b);
        for (path, name) in [(&wa, "a"), (&wb, "b")] {
            std::fs::write(path.join(name), name).unwrap();
            git(path, &["add", "--all"]).unwrap();
            git(path, &["commit", "-m", name]).unwrap();
        }
        merge(&project, &a).unwrap();
        incorporate_main(&wb).unwrap();
        verify(&wb, &["test -f a && test -f b".into()]).unwrap();
        merge(&project, &b).unwrap();
        cleanup(&project, &wa).unwrap();
        cleanup(&project, &wb).unwrap();
        assert!(project.join("a").exists() && project.join("b").exists());
        assert!(!wa.exists() && !wb.exists());
    }

    #[test]
    fn conflict_preserves_committed_work_and_main() {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        std::fs::create_dir(&project).unwrap();
        std::fs::write(project.join("file"), "original\n").unwrap();
        let (a, wa, _) = create_worktree(&project, "a").unwrap();
        let (_, wb, _) = create_worktree(&project, "b").unwrap();
        for (path, text) in [(&wa, "a\n"), (&wb, "b\n")] {
            std::fs::write(path.join("file"), text).unwrap();
            git(path, &["add", "--all"]).unwrap();
            git(path, &["commit", "-m", "change"]).unwrap();
        }
        merge(&project, &a).unwrap();
        assert!(incorporate_main(&wb).is_err());
        assert_eq!(std::fs::read_to_string(wb.join("file")).unwrap(), "b\n");
        assert_eq!(
            std::fs::read_to_string(project.join("file")).unwrap(),
            "a\n"
        );
        assert!(verify(&wb, &["exit 1".into()]).is_err());
    }
}
