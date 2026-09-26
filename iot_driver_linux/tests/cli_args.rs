//! Argument-level checks for the CLI surface.
//!
//! These run the real binary, so they cover what clap actually accepts — the
//! thing a user hits when a flag is misspelled or combined with one it cannot
//! coexist with. They must not touch hardware: every case here is rejected
//! during parsing, before a device is opened.

use std::process::Command;

fn driver() -> Command {
    Command::new(env!("CARGO_BIN_EXE_iot_driver"))
}

#[test]
fn standard_needs_both_values() {
    // One value is not a mapping: release would have nothing to derive from.
    let out = driver()
        .args(["set-key-trigger", "A", "--standard", "1.5"])
        .output()
        .expect("run driver");
    assert!(!out.status.success(), "one value should be rejected");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("2 values") || err.contains("<ACTUATION_MM> <SENSITIVITY_MM>"),
        "unhelpful error: {err}"
    );
}

#[test]
fn standard_conflicts_with_the_fields_it_derives() {
    // --standard computes release and both sensitivities, so pairing it with the
    // individual fields would silently discard one of the two.
    for extra in [
        vec!["--actuation", "1.5"],
        vec!["--release", "1.1"],
        vec!["--rt-press", "0.4"],
        vec!["--rt-lift", "0.4"],
        vec!["--rt", "true"],
    ] {
        let mut args = vec!["set-key-trigger", "A", "--standard", "1.5", "0.4"];
        args.extend(extra.iter().copied());
        let out = driver().args(&args).output().expect("run driver");
        assert!(
            !out.status.success(),
            "--standard with {extra:?} should be rejected"
        );
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        assert!(
            err.contains("cannot be used with") || err.contains("conflict"),
            "expected a conflict error for {extra:?}, got: {err}"
        );
    }
}

#[test]
fn standard_is_documented_in_help() {
    let out = driver()
        .args(["set-key-trigger", "--help"])
        .output()
        .expect("run");
    let help = String::from_utf8_lossy(&out.stdout);
    assert!(help.contains("--standard"), "flag missing from help");
    // Both value names, so the argument order is unambiguous.
    assert!(help.contains("ACTUATION_MM") && help.contains("SENSITIVITY_MM"));
    // And the relationship, since that is the part that is easy to get wrong.
    assert!(
        help.contains("actuation - sensitivity"),
        "help should state how release is derived"
    );
}
