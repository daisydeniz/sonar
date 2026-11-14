//! `sonar license` end to end in a temp SONAR_HOME. Activation tests use the
//! committed DEV secret key, so they only apply to builds that trust the dev
//! public key (no SONAR_LICENSE_PUBLIC_KEY at build time).

use std::path::Path;
use std::process::{Command, Output};

const DEV_SECRET: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../sonar-core/dev/license-dev-secret.key"
);

fn sonar(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_sonar"))
        .args(args)
        .env("SONAR_HOME", home)
        .env_remove("RUST_LOG")
        .output()
        .expect("spawn sonar")
}

fn ok(out: &Output) -> String {
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout.clone()).expect("utf-8")
}

fn issue(home: &Path, extra: &[&str]) -> Output {
    let mut a = vec![
        "license",
        "issue",
        "--secret-key",
        DEV_SECRET,
        "--licensee",
        "Ada <ada@example.com>",
        "--license-id",
        "ord_42",
    ];
    a.extend_from_slice(extra);
    sonar(home, &a)
}

fn status_json(home: &Path) -> serde_json::Value {
    serde_json::from_str(&ok(&sonar(home, &["license", "status", "--json"]))).expect("json")
}

#[test]
fn status_starts_a_trial() {
    let home = tempfile::tempdir().expect("tmp");
    let v = status_json(home.path());
    assert_eq!(v["state"], "trial");
    assert_eq!(v["days_left"], 14);
    assert!(home.path().join("license.json").exists());
    assert!(ok(&sonar(home.path(), &["license", "status"])).contains("14 day(s) left"));
}

#[test]
fn issue_activate_status_deactivate() {
    let home = tempfile::tempdir().expect("tmp");
    let key = ok(&issue(home.path(), &[])).trim().to_owned();
    assert!(key.starts_with("SONAR1-"), "{key}");
    if status_json(home.path())["dev_key"] != true {
        return; // release-key build: dev-signed keys are (rightly) rejected
    }

    // A wrapped key given as several words is accepted.
    let (a, b) = key.split_at(30);
    ok(&sonar(home.path(), &["license", "activate", a, b]));
    let v = status_json(home.path());
    assert_eq!(v["state"], "licensed");
    assert_eq!(v["licensee"], "Ada <ada@example.com>");
    assert_eq!(v["license_id"], "ord_42");
    assert_eq!(v["kind"], "perpetual");

    // A bad key fails and keeps the license.
    let bad = sonar(home.path(), &["license", "activate", "SONAR1-nope.nope"]);
    assert!(!bad.status.success());
    assert!(String::from_utf8_lossy(&bad.stderr).contains("not a valid Sonar license key"));
    assert_eq!(status_json(home.path())["state"], "licensed");

    ok(&sonar(home.path(), &["license", "deactivate"]));
    assert_eq!(status_json(home.path())["state"], "trial");
}

#[test]
fn expired_subscription_cannot_be_activated() {
    let home = tempfile::tempdir().expect("tmp");
    let key = ok(&issue(
        home.path(),
        &[
            "--kind",
            "subscription",
            "--issued",
            "2020-01-01",
            "--expires",
            "2020-12-31",
        ],
    ));
    if status_json(home.path())["dev_key"] != true {
        return;
    }
    let out = sonar(home.path(), &["license", "activate", key.trim()]);
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("expired on 2020-12-31"));
}

#[test]
fn issue_validates_its_input() {
    let home = tempfile::tempdir().expect("tmp");
    // Subscription without expiry, bad kind, bad date.
    for extra in [
        &["--kind", "subscription"][..],
        &["--kind", "lifetime"],
        &["--expires", "2026-02-30"],
    ] {
        assert!(!issue(home.path(), extra).status.success(), "{extra:?}");
    }
}

#[test]
fn keygen_writes_a_private_file_and_refuses_to_overwrite() {
    use std::os::unix::fs::PermissionsExt;
    let home = tempfile::tempdir().expect("tmp");
    let out = home.path().join("secret.key");
    let out_arg = out.to_str().expect("utf-8");
    let public = ok(&sonar(
        home.path(),
        &["license", "keygen", "--out", out_arg],
    ));
    assert_eq!(public.trim().len(), 43, "base64url of 32 bytes: {public}");
    let mode = std::fs::metadata(&out).expect("meta").permissions().mode();
    assert_eq!(mode & 0o777, 0o600);
    let before = std::fs::read(&out).expect("read");
    let again = sonar(home.path(), &["license", "keygen", "--out", out_arg]);
    assert!(!again.status.success());
    assert_eq!(std::fs::read(&out).expect("read"), before);

    // The generated secret can issue keys.
    let issued = sonar(
        home.path(),
        &[
            "license",
            "issue",
            "--secret-key",
            out_arg,
            "--licensee",
            "x",
            "--license-id",
            "1",
        ],
    );
    assert!(ok(&issued).starts_with("SONAR1-"));
}
