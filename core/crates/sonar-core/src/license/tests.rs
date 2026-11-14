//! Tests of key verification, trial/status logic and activation.

use std::fs;

use ed25519_dalek::Signer;

use super::verify::b64_encode;
use super::*;

const DAY: i64 = 86_400;
/// 2026-01-01 00:00:00 UTC.
const T0: i64 = 1_767_225_600;

fn verifier() -> SignedKeyVerifier {
    SignedKeyVerifier::new(dev::signing_key().verifying_key())
}

fn d(s: &str) -> Date {
    Date::parse(s).expect("date")
}

fn info(kind: LicenseKind, expires: Option<&str>) -> LicenseInfo {
    LicenseInfo {
        licensee: "Ada Lovelace <ada@example.com>".into(),
        license_id: "ord_123".into(),
        issued: d("2026-01-01"),
        kind,
        expires: expires.map(d),
    }
}

fn perpetual_key() -> String {
    issue(&dev::signing_key(), &info(LicenseKind::Perpetual, None)).expect("issue")
}

fn sub_key(expires: &str) -> String {
    issue(
        &dev::signing_key(),
        &info(LicenseKind::Subscription, Some(expires)),
    )
    .expect("issue")
}

/// Signs arbitrary payload bytes with the dev key (to craft odd keys).
fn raw_key(payload: &str) -> String {
    let sig = dev::signing_key().sign(payload.as_bytes());
    format!(
        "{KEY_PREFIX}{}.{}",
        b64_encode(payload.as_bytes()),
        b64_encode(&sig.to_bytes())
    )
}

#[test]
fn dev_public_key_constant_matches_the_committed_secret() {
    let derived = b64_encode(dev::signing_key().verifying_key().as_bytes());
    assert_eq!(derived, DEV_PUBLIC_KEY);
}

#[test]
fn valid_key_verifies_and_roundtrips_its_fields() {
    let got = verifier()
        .verify(&perpetual_key(), d("2030-01-01"))
        .expect("valid");
    assert_eq!(got, info(LicenseKind::Perpetual, None));
}

#[test]
fn whitespace_in_a_pasted_key_is_ignored() {
    let key = perpetual_key();
    let (a, b) = key.split_at(20);
    let pasted = format!("  {a}\n\t{b}\r\n ");
    assert!(verifier().verify(&pasted, d("2026-06-01")).is_ok());
}

#[test]
fn tampered_payload_or_signature_fails() {
    let key = perpetual_key();
    let body = key.strip_prefix(KEY_PREFIX).expect("prefix");
    let (p, s) = body.split_once('.').expect("dot");
    let payload = String::from_utf8(
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, p).expect("b64"),
    )
    .expect("utf8");
    let forged = payload.replace("ord_123", "ord_999");
    let tampered = format!("{KEY_PREFIX}{}.{s}", b64_encode(forged.as_bytes()));
    assert!(matches!(
        verifier().verify(&tampered, d("2026-06-01")),
        Err(LicenseError::BadSignature)
    ));
    // Flip one bit of the signature.
    let mut sig =
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, s).expect("sig");
    sig[10] ^= 1;
    let bad_sig = format!("{KEY_PREFIX}{p}.{}", b64_encode(&sig));
    assert!(matches!(
        verifier().verify(&bad_sig, d("2026-06-01")),
        Err(LicenseError::BadSignature)
    ));
}

#[test]
fn key_signed_by_another_key_fails() {
    let other = parse_secret_key(&generate_keypair().expect("keypair").secret).expect("secret");
    let key = issue(&other, &info(LicenseKind::Perpetual, None)).expect("issue");
    assert!(matches!(
        verifier().verify(&key, d("2026-06-01")),
        Err(LicenseError::BadSignature)
    ));
}

#[test]
fn wrong_product_and_unsupported_version() {
    let base = r#""licensee":"x","license_id":"1","issued":"2026-01-01","kind":"perpetual""#;
    let other = raw_key(&format!(r#"{{"v":1,"product":"other",{base}}}"#));
    assert!(matches!(
        verifier().verify(&other, d("2026-06-01")),
        Err(LicenseError::WrongProduct { found }) if found == "other"
    ));
    let v2 = raw_key(&format!(r#"{{"v":2,"product":"sonar",{base}}}"#));
    assert!(matches!(
        verifier().verify(&v2, d("2026-06-01")),
        Err(LicenseError::UnsupportedVersion { found: 2 })
    ));
    let missing_fields = raw_key(r#"{"v":1,"product":"sonar"}"#);
    assert!(matches!(
        verifier().verify(&missing_fields, d("2026-06-01")),
        Err(LicenseError::Malformed { .. })
    ));
}

#[test]
fn subscription_is_valid_through_its_last_day_then_expired() {
    let key = sub_key("2026-12-31");
    assert!(verifier().verify(&key, d("2026-12-31")).is_ok());
    match verifier().verify(&key, d("2027-01-01")) {
        Err(LicenseError::Expired { date }) => assert_eq!(date, d("2026-12-31")),
        other => panic!("{other:?}"),
    }
    // `inspect` still describes it.
    assert!(verifier().inspect(&key).is_ok());
}

#[test]
fn issue_rejects_impossible_licenses() {
    let sk = dev::signing_key();
    for bad in [
        info(LicenseKind::Subscription, None),
        info(LicenseKind::Subscription, Some("2025-12-31")),
        LicenseInfo {
            licensee: " ".into(),
            ..info(LicenseKind::Perpetual, None)
        },
    ] {
        assert!(matches!(
            issue(&sk, &bad),
            Err(LicenseError::InvalidIssue { .. })
        ));
    }
}

#[test]
fn garbage_truncated_and_huge_input_never_panics() {
    let good = perpetual_key();
    let mut inputs: Vec<String> = vec![
        String::new(),
        " ".into(),
        KEY_PREFIX.into(),
        format!("{KEY_PREFIX}."),
        format!("{KEY_PREFIX}.."),
        format!("{KEY_PREFIX}!!!.???"),
        "SONAR1-é.ü".into(),
        "\u{0}\u{1}\u{ffff}".into(),
        "x".repeat(1_000_000),
        format!("{KEY_PREFIX}{}", "A".repeat(1_000_000)),
        format!("{KEY_PREFIX}{}.{}", "A".repeat(100), "B".repeat(86)),
    ];
    for cut in 0..good.len() {
        if good.is_char_boundary(cut) {
            inputs.push(good[..cut].to_owned());
        }
    }
    for i in 0..good.len() {
        let mut b = good.clone().into_bytes();
        b[i] = b'#';
        inputs.push(String::from_utf8_lossy(&b).into_owned());
    }
    for input in &inputs {
        assert!(verifier().verify(input, d("2026-06-01")).is_err());
    }
}

#[test]
fn key_text_parsing_of_signing_keys_is_strict_and_tolerant() {
    let kp = generate_keypair().expect("keypair");
    assert!(parse_public_key(&kp.public).is_ok());
    assert!(parse_secret_key(&format!("{}\n", kp.secret)).is_ok());
    for bad in ["", "abc", "!!!!", &"A".repeat(100)] {
        assert!(matches!(
            parse_public_key(bad),
            Err(LicenseError::BadSigningKey { .. })
        ));
    }
    assert_ne!(
        generate_keypair().expect("a").secret,
        generate_keypair().expect("b").secret
    );
}

// ---- trial and status --------------------------------------------------

fn st(dir: &std::path::Path, now: i64) -> LicenseStatus {
    status_with(&verifier(), dir, now).expect("status")
}

fn days_left(s: &LicenseStatus) -> Option<u32> {
    match s {
        LicenseStatus::Trial { days_left, .. } => Some(*days_left),
        _ => None,
    }
}

#[test]
fn trial_counts_down_and_expires_on_day_14() {
    let dir = tempfile::tempdir().expect("tmp");
    let s0 = st(dir.path(), T0);
    assert_eq!(
        s0,
        LicenseStatus::Trial {
            days_left: 14,
            ends_at: T0 + 14 * DAY
        }
    );
    assert_eq!(days_left(&st(dir.path(), T0 + DAY)), Some(13));
    assert_eq!(days_left(&st(dir.path(), T0 + 13 * DAY)), Some(1));
    assert_eq!(days_left(&st(dir.path(), T0 + 14 * DAY - 1)), Some(1));
    assert_eq!(
        st(dir.path(), T0 + 14 * DAY),
        LicenseStatus::TrialExpired {
            ended_at: T0 + 14 * DAY
        }
    );
    assert!(matches!(
        st(dir.path(), T0 + 15 * DAY),
        LicenseStatus::TrialExpired { .. }
    ));
}

#[test]
fn moving_the_clock_back_never_adds_trial_days() {
    let dir = tempfile::tempdir().expect("tmp");
    st(dir.path(), T0);
    assert!(matches!(
        st(dir.path(), T0 + 20 * DAY),
        LicenseStatus::TrialExpired { .. }
    ));
    // Clock set back to the start: still expired.
    assert!(matches!(
        st(dir.path(), T0),
        LicenseStatus::TrialExpired { .. }
    ));
    // Rolling back within the trial does not give days back either.
    let dir = tempfile::tempdir().expect("tmp");
    st(dir.path(), T0);
    assert_eq!(days_left(&st(dir.path(), T0 + 10 * DAY)), Some(4));
    assert_eq!(days_left(&st(dir.path(), T0 + 2 * DAY)), Some(4));
}

#[test]
fn activate_licenses_and_survives_restart() {
    let dir = tempfile::tempdir().expect("tmp");
    st(dir.path(), T0);
    let got = activate_with(&verifier(), dir.path(), &perpetual_key(), T0 + DAY).expect("ok");
    assert_eq!(got.license_id, "ord_123");
    // Far past the trial: still licensed.
    match st(dir.path(), T0 + 400 * DAY) {
        LicenseStatus::Licensed { info } => assert_eq!(info.kind, LicenseKind::Perpetual),
        other => panic!("{other:?}"),
    }
}

#[test]
fn expired_subscription_reports_license_expired() {
    let dir = tempfile::tempdir().expect("tmp");
    st(dir.path(), T0);
    activate_with(&verifier(), dir.path(), &sub_key("2026-03-01"), T0).expect("ok");
    assert!(matches!(
        st(dir.path(), T0 + 30 * DAY),
        LicenseStatus::Licensed { .. }
    ));
    match st(dir.path(), T0 + 100 * DAY) {
        LicenseStatus::LicenseExpired { info } => {
            assert_eq!(info.expires, Some(d("2026-03-01")));
        }
        other => panic!("{other:?}"),
    }
    // An already expired key cannot be activated.
    let fresh = tempfile::tempdir().expect("tmp");
    assert!(matches!(
        activate_with(
            &verifier(),
            fresh.path(),
            &sub_key("2026-03-01"),
            T0 + 100 * DAY
        ),
        Err(LicenseError::Expired { .. })
    ));
}

#[test]
fn invalid_activation_keeps_the_stored_key() {
    let dir = tempfile::tempdir().expect("tmp");
    activate_with(&verifier(), dir.path(), &perpetual_key(), T0).expect("ok");
    let lapsed = issue(
        &dev::signing_key(),
        &LicenseInfo {
            issued: d("2019-01-01"),
            ..info(LicenseKind::Subscription, Some("2020-01-01"))
        },
    )
    .expect("issue");
    for bad in ["garbage", "SONAR1-abc.def", &lapsed] {
        assert!(activate_with(&verifier(), dir.path(), bad, T0 + DAY).is_err());
    }
    assert!(matches!(
        st(dir.path(), T0 + 500 * DAY),
        LicenseStatus::Licensed { .. }
    ));
}

#[test]
fn deactivate_removes_key_but_keeps_trial_clock() {
    let dir = tempfile::tempdir().expect("tmp");
    st(dir.path(), T0);
    activate_with(&verifier(), dir.path(), &perpetual_key(), T0 + 3 * DAY).expect("ok");
    deactivate(dir.path()).expect("deactivate");
    // Trial started at T0: far in the future it is expired, not restarted.
    assert!(matches!(
        st(dir.path(), T0 + 40 * DAY),
        LicenseStatus::TrialExpired { .. }
    ));
}

#[test]
fn deactivate_without_state_is_a_noop() {
    let dir = tempfile::tempdir().expect("tmp");
    deactivate(dir.path()).expect("deactivate");
    assert!(!dir.path().join(STATE_FILE).exists());
}

#[test]
fn stored_key_that_no_longer_verifies_falls_back_to_the_trial() {
    let dir = tempfile::tempdir().expect("tmp");
    activate_with(&verifier(), dir.path(), &perpetual_key(), T0).expect("ok");
    let other = SignedKeyVerifier::new(
        parse_public_key(&generate_keypair().expect("kp").public).expect("pk"),
    );
    assert_eq!(
        status_with(&other, dir.path(), T0 + DAY).expect("status"),
        LicenseStatus::Trial {
            days_left: 13,
            ends_at: T0 + 14 * DAY
        }
    );
    assert!(matches!(
        status_with(&other, dir.path(), T0 + 30 * DAY).expect("status"),
        LicenseStatus::TrialExpired { .. }
    ));
}

#[test]
fn corrupt_state_file_is_never_an_error_and_is_rewritten() {
    let dir = tempfile::tempdir().expect("tmp");
    let file = dir.path().join(STATE_FILE);
    fs::write(&file, b"\xff\xfe garbage").expect("write");
    let s = status_with(&verifier(), dir.path(), now_unix()).expect("no error");
    assert!(matches!(s, LicenseStatus::Trial { .. }));
    // Rewritten as valid JSON.
    let v: serde_json::Value =
        serde_json::from_slice(&fs::read(&file).expect("read")).expect("json");
    assert!(v.get("trial_started").is_some());
}

#[test]
fn status_does_not_rewrite_the_file_on_every_call() {
    let dir = tempfile::tempdir().expect("tmp");
    st(dir.path(), T0);
    let file = dir.path().join(STATE_FILE);
    let before = fs::read(&file).expect("read");
    st(dir.path(), T0 + 5);
    assert_eq!(fs::read(&file).expect("read"), before);
    st(dir.path(), T0 + 3600);
    assert_ne!(fs::read(&file).expect("read"), before);
}

#[test]
fn builtin_verifier_uses_the_dev_key_in_dev_builds() {
    if uses_dev_key() {
        assert!(SignedKeyVerifier::builtin().is_ok());
        let key = perpetual_key();
        let dir = tempfile::tempdir().expect("tmp");
        assert!(activate(dir.path(), &key, T0).is_ok());
        assert!(matches!(
            status(dir.path(), T0).expect("status"),
            LicenseStatus::Licensed { .. }
        ));
    }
}
