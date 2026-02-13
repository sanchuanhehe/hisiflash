//! Integration tests for core CLI contract behavior.

use predicates::prelude::*;
use std::fs;
use tempfile::tempdir;

fn cli_cmd() -> assert_cmd::Command {
    assert_cmd::cargo::cargo_bin_cmd!("hisiflash")
}

#[test]
fn help_exits_zero_and_writes_stdout_only() {
    let mut cmd = cli_cmd();
    cmd.arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("hisiflash"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn short_help_exits_zero_and_writes_stdout_only() {
    let mut cmd = cli_cmd();
    cmd.arg("-h")
        .assert()
        .success()
        .stdout(predicate::str::contains("hisiflash"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn version_exits_zero_and_writes_stdout_only() {
    let mut cmd = cli_cmd();
    cmd.arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("hisiflash"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn short_version_exits_zero_and_writes_stdout_only() {
    let mut cmd = cli_cmd();
    cmd.arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("hisiflash"))
        .stderr(predicate::str::is_empty());
}

#[test]
fn list_ports_json_writes_machine_output_to_stdout_only() {
    let mut cmd = cli_cmd();
    let output = cmd
        .args(["list-ports", "--json"])
        .assert()
        .success()
        .stderr(predicate::str::is_empty())
        .get_output()
        .clone();

    let stdout = String::from_utf8(output.stdout).expect("stdout should be utf-8");
    let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid json expected");
    assert!(
        parsed.is_array(),
        "list-ports --json should return an array"
    );
}

#[test]
fn info_json_error_keeps_stdout_clean() {
    let mut cmd = cli_cmd();
    cmd.args(["info", "--json", "/tmp/not_exists_for_contract_test.fwpkg"])
        .assert()
        .failure()
        .stdout(predicate::str::is_empty())
        .stderr(predicate::str::contains("Error:"));
}

#[test]
fn non_interactive_flash_with_multiple_firmwares_fails_fast() {
    let dir = tempdir().expect("tempdir should be created");
    let fw_a = dir.path().join("a.fwpkg");
    let fw_b = dir.path().join("b.fwpkg");
    fs::write(&fw_a, b"dummy").expect("write a.fwpkg");
    fs::write(&fw_b, b"dummy").expect("write b.fwpkg");

    let mut cmd = cli_cmd();
    cmd.current_dir(dir.path())
        .arg("--non-interactive")
        .arg("flash")
        .assert()
        .failure()
        .stderr(predicate::str::contains("multiple").or(predicate::str::contains("多个")));
}
