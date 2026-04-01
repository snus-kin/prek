mod common;
use anyhow::Result;
use assert_cmd::assert::OutputAssertExt;
use assert_fs::prelude::*;
use std::path::PathBuf;

use crate::common::{TestContext, cmd_snapshot, git_cmd};
use assert_fs::fixture::ChildPath;
use prek_consts::PRE_COMMIT_HOOKS_YAML;

fn create_hook_repo(context: &TestContext, repo_name: &str) -> Result<PathBuf> {
    let repo_dir = context.home_dir().child(format!("test-repos/{repo_name}"));
    repo_dir.create_dir_all()?;

    git_cmd(&repo_dir).arg("init").assert().success();

    // Configure the author specifically for this hook repository
    git_cmd(&repo_dir)
        .arg("config")
        .arg("user.name")
        .arg("Prek Test")
        .assert()
        .success();
    git_cmd(&repo_dir)
        .arg("config")
        .arg("user.email")
        .arg("test@prek.dev")
        .assert()
        .success();
    // Disable autocrlf for test consistency
    git_cmd(&repo_dir)
        .arg("config")
        .arg("core.autocrlf")
        .arg("false")
        .assert()
        .success();

    repo_dir
        .child(PRE_COMMIT_HOOKS_YAML)
        .write_str(indoc::indoc! {r#"
        - id: test-hook
          name: Test Hook
          entry: echo
          language: system
          files: "\\.txt$"
        - id: another-hook
          name: Another Hook
          entry: python3 -c "print('hello')"
          language: python
    "#})?;

    // Add a dummy setup.py to make it an installable Python package
    repo_dir
        .child("setup.py")
        .write_str("from setuptools import setup; setup(name='dummy-pkg', version='0.0.1')")?;

    git_cmd(&repo_dir).arg("add").arg(".").assert().success();

    git_cmd(&repo_dir)
        .arg("commit")
        .arg("-m")
        .arg("Initial commit")
        .assert()
        .success();

    Ok(repo_dir.to_path_buf())
}

// Helper for a repo with a hook that is designed to fail
fn create_failing_hook_repo(context: &TestContext, repo_name: &str) -> Result<PathBuf> {
    let repo_dir = context.home_dir().child(format!("test-repos/{repo_name}"));
    repo_dir.create_dir_all()?;

    git_cmd(&repo_dir).arg("init").assert().success();
    git_cmd(&repo_dir)
        .arg("config")
        .arg("user.name")
        .arg("Prek Test")
        .assert()
        .success();
    git_cmd(&repo_dir)
        .arg("config")
        .arg("user.email")
        .arg("test@prek.dev")
        .assert()
        .success();
    // Disable autocrlf for test consistency
    git_cmd(&repo_dir)
        .arg("config")
        .arg("core.autocrlf")
        .arg("false")
        .assert()
        .success();

    repo_dir
        .child(PRE_COMMIT_HOOKS_YAML)
        .write_str(indoc::indoc! {r#"
        - id: failing-hook
          name: Always Fail
          entry: "false"
          language: system
        "#})?;

    git_cmd(&repo_dir).arg("add").arg(".").assert().success();

    git_cmd(&repo_dir)
        .arg("commit")
        .arg("-m")
        .arg("Initial commit")
        .assert()
        .success();

    Ok(repo_dir.to_path_buf())
}

#[test]
fn try_repo_basic() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    let repo_path = create_hook_repo(&context, "try-repo-basic")?;

    let mut filters = context.filters();
    filters.extend([(r"[a-f0-9]{40}", "[COMMIT_SHA]"), ("'", "\"")]);

    cmd_snapshot!(filters, context.try_repo().arg(&repo_path).arg("--skip").arg("another-hook"), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "[HOME]/test-repos/try-repo-basic"
    rev = "[COMMIT_SHA]"
    hooks = [
      { id = "test-hook" },
    ]

    Test Hook................................................................Passed

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_failing_hook() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    let repo_path = create_failing_hook_repo(&context, "try-repo-failing")?;

    let mut filters = context.filters();
    filters.extend([(r"[a-f0-9]{40}", "[COMMIT_SHA]"), ("'", "\"")]);

    cmd_snapshot!(filters, context.try_repo().arg(&repo_path), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "[HOME]/test-repos/try-repo-failing"
    rev = "[COMMIT_SHA]"
    hooks = [
      { id = "failing-hook" },
    ]

    Always Fail..............................................................Failed
    - hook id: failing-hook
    - exit code: 1

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_specific_hook() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    let repo_path = create_hook_repo(&context, "try-repo-specific-hook")?;

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    let mut filters = context.filters();
    filters.extend([(r"[a-f0-9]{40}", "[COMMIT_SHA]"), ("'", "\"")]);

    cmd_snapshot!(filters, context.try_repo().arg(&repo_path).arg("another-hook"), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "[HOME]/test-repos/try-repo-specific-hook"
    rev = "[COMMIT_SHA]"
    hooks = [
      { id = "another-hook" },
    ]

    Another Hook.............................................................Passed

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_specific_rev() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    let repo_path = create_hook_repo(&context, "try-repo-specific-rev")?;

    let initial_rev = git_cmd(&repo_path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()?
        .stdout;
    let initial_rev = String::from_utf8_lossy(&initial_rev).trim().to_string();

    // Make a new commit
    ChildPath::new(&repo_path)
        .child(PRE_COMMIT_HOOKS_YAML)
        .write_str(indoc::indoc! {r"
        - id: new-hook
          name: New Hook
          entry: echo new
          language: system
        "})?;
    git_cmd(&repo_path).arg("add").arg(".").assert().success();
    git_cmd(&repo_path)
        .arg("commit")
        .arg("-m")
        .arg("second")
        .assert()
        .success();

    let mut filters = context.filters();
    filters.extend([
        (r"[a-f0-9]{40}", "[COMMIT_SHA]"),
        (&initial_rev, "[COMMIT_SHA]"),
        ("'", "\""),
    ]);

    cmd_snapshot!(filters, context.try_repo().arg(&repo_path)
        .arg("--ref")
        .arg(&initial_rev), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "[HOME]/test-repos/try-repo-specific-rev"
    rev = "[COMMIT_SHA]"
    hooks = [
      { id = "test-hook" },
      { id = "another-hook" },
    ]

    Test Hook................................................................Passed
    Another Hook.............................................................Passed

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_uncommitted_changes() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    let repo_path = create_hook_repo(&context, "try-repo-uncommitted")?;

    // Make uncommitted changes
    ChildPath::new(&repo_path)
        .child(PRE_COMMIT_HOOKS_YAML)
        .write_str(indoc::indoc! {r"
        - id: uncommitted-hook
          name: Uncommitted Hook
          entry: echo uncommitted
          language: system
        "})?;
    ChildPath::new(&repo_path)
        .child("new-file.txt")
        .write_str("new")?;
    git_cmd(&repo_path)
        .arg("add")
        .arg("new-file.txt")
        .assert()
        .success();

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    let mut filters = context.filters();
    filters.extend([
        (r"try-repo-[^/\\]+", "[REPO]"),
        (r"[a-f0-9]{40}", "[COMMIT_SHA]"),
        ("'", "\""),
    ]);

    cmd_snapshot!(filters, context.try_repo().arg(&repo_path), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "[HOME]/scratch/[REPO]/shadow-repo"
    rev = "[COMMIT_SHA]"
    hooks = [
      { id = "uncommitted-hook" },
    ]

    Uncommitted Hook.........................................................Passed

    ----- stderr -----
    warning: Creating temporary repo with uncommitted changes...
    "#);

    Ok(())
}

#[test]
fn try_repo_relative_path() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    let _repo_path = create_hook_repo(&context, "try-repo-relative")?;
    let relative_path = "../home/test-repos/try-repo-relative".to_string();

    let mut filters = context.filters();
    filters.extend([(r"[a-f0-9]{40}", "[COMMIT_SHA]")]);

    cmd_snapshot!(filters, context.try_repo().arg(&relative_path), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "../home/test-repos/try-repo-relative"
    rev = "[COMMIT_SHA]"
    hooks = [
      { id = "test-hook" },
      { id = "another-hook" },
    ]

    Test Hook................................................................Passed
    Another Hook.............................................................Passed

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_builtin() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    // Create a file with trailing whitespace to trigger the hook
    context
        .work_dir()
        .child("test.txt")
        .write_str("test content  \n")?;
    context.git_add(".");

    cmd_snapshot!(context.filters(), context.try_repo().arg("builtin").arg("trailing-whitespace"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "builtin"
    hooks = [
      { id = "trailing-whitespace" },
    ]

    trim trailing whitespace.................................................Failed
    - hook id: trailing-whitespace
    - exit code: 1
    - files were modified by this hook

      Fixing test.txt

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_builtin_multiple_hooks() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    // Create files that will trigger multiple builtin hooks
    context
        .work_dir()
        .child("test.txt")
        .write_str("test content  \n")?; // trailing whitespace
    context
        .work_dir()
        .child("test.json")
        .write_str(r#"{"key": "value"}"#)?; // no newline at end
    context.git_add(".");

    cmd_snapshot!(context.filters(), context.try_repo().arg("builtin")
        .arg("trailing-whitespace")
        .arg("end-of-file-fixer"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "builtin"
    hooks = [
      { id = "end-of-file-fixer" },
      { id = "trailing-whitespace" },
    ]

    fix end of files.........................................................Failed
    - hook id: end-of-file-fixer
    - exit code: 1
    - files were modified by this hook

      Fixing test.json
    trim trailing whitespace.................................................Failed
    - hook id: trailing-whitespace
    - exit code: 1
    - files were modified by this hook

      Fixing test.txt

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_builtin_with_includes() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    context
        .work_dir()
        .child("test.txt")
        .write_str("test content  \n")?;
    context.git_add(".");

    cmd_snapshot!(context.filters(), context.try_repo().arg("builtin")
        .arg("trailing-whitespace"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "builtin"
    hooks = [
      { id = "trailing-whitespace" },
    ]

    trim trailing whitespace.................................................Failed
    - hook id: trailing-whitespace
    - exit code: 1
    - files were modified by this hook

      Fixing test.txt

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_meta() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    // Create a basic config file for meta hooks to check
    let repo_path = create_hook_repo(&context, "meta-test-repo")?;
    context.write_pre_commit_config(&format!(
        indoc::indoc! {r"
        repos:
          - repo: {}
            rev: HEAD
            hooks:
              - id: test-hook
    "},
        repo_path.display()
    ));
    context.git_add(".");

    let mut filters = context.filters();
    filters.push((r"- duration: \d+\.\d+s", "- duration: [TIME]"));
    cmd_snapshot!(filters, context.try_repo().arg("meta").arg("identity"), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "meta"
    hooks = [
      { id = "identity" },
    ]

    identity.................................................................Passed
    - hook id: identity
    - duration: [TIME]

      .pre-commit-config.yaml

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_meta_check_hooks_apply() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    // Create a config file with a hook that doesn't apply to any files
    let repo_path = create_hook_repo(&context, "meta-check-hooks-repo")?;

    // Get the commit SHA to avoid mutable reference warning
    let commit_sha = git_cmd(&repo_path)
        .arg("rev-parse")
        .arg("HEAD")
        .output()?
        .stdout;
    let commit_sha = String::from_utf8_lossy(&commit_sha).trim().to_string();

    context.write_pre_commit_config(&format!(
        indoc::indoc! {r"
        repos:
          - repo: {}
            rev: {}
            hooks:
              - id: test-hook
                files: '\.nonexistent$'
    "},
        repo_path.display(),
        commit_sha
    ));
    context.git_add(".");

    cmd_snapshot!(context.filters(), context.try_repo().arg("meta").arg("check-hooks-apply"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "meta"
    hooks = [
      { id = "check-hooks-apply" },
    ]

    Check hooks apply........................................................Failed
    - hook id: check-hooks-apply
    - exit code: 1

      test-hook does not apply to this repository

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_meta_with_skip() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    let repo_path = create_hook_repo(&context, "meta-skip-repo")?;
    context.write_pre_commit_config(&format!(
        indoc::indoc! {r"
        repos:
          - repo: {}
            rev: HEAD
            hooks:
              - id: test-hook
    "},
        repo_path.display()
    ));
    context.git_add(".");

    let mut filters = context.filters();
    filters.push((r"- duration: \d+\.\d+s", "- duration: [TIME]"));
    cmd_snapshot!(filters, context.try_repo().arg("meta")
        .arg("--skip")
        .arg("check-hooks-apply")
        .arg("--skip")
        .arg("check-useless-excludes"), @r#"
    success: true
    exit_code: 0
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "meta"
    hooks = [
      { id = "identity" },
    ]

    identity.................................................................Passed
    - hook id: identity
    - duration: [TIME]

      .pre-commit-config.yaml

    ----- stderr -----
    "#);

    Ok(())
}

#[test]
fn try_repo_builtin_unknown_hook() -> Result<()> {
    let context = TestContext::new();
    context.init_project();

    context.work_dir().child("test.txt").write_str("test")?;
    context.git_add(".");

    cmd_snapshot!(context.filters(), context.try_repo().arg("builtin").arg("nonexistent-hook"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "builtin"
    hooks = [
    ]


    ----- stderr -----
    error: No hooks found after filtering with the given selectors
    "#);

    Ok(())
}

#[test]
fn try_repo_meta_unknown_hook() {
    let context = TestContext::new();
    context.init_project();

    context.write_pre_commit_config(indoc::indoc! {r"
        repos:
          - repo: https://github.com/example/example
            rev: v1.0.0
            hooks:
              - id: example-hook
    "});
    context.git_add(".");

    cmd_snapshot!(context.filters(), context.try_repo().arg("meta").arg("nonexistent-hook"), @r#"
    success: false
    exit_code: 1
    ----- stdout -----
    Using generated `prek.toml`:
    [[repos]]
    repo = "meta"
    hooks = [
    ]


    ----- stderr -----
    error: No hooks found after filtering with the given selectors
    "#);
}
