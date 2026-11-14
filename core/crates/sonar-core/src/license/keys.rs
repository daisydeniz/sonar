//! Signing keys: the public key this build trusts, key generation, parsing,
//! and issuing license keys (for the founder or a fulfilment server).

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};

use super::verify::{b64_decode_lenient, b64_encode, Payload};
use super::{LicenseError, LicenseInfo, LicenseKind, KEY_PREFIX, PAYLOAD_VERSION, PRODUCT};

/// Public key (base64) baked in at build time, if the release build set
/// `SONAR_LICENSE_PUBLIC_KEY`. Cargo tracks `option_env!`, so changing the
/// variable rebuilds this crate.
const BUILD_PUBLIC_KEY: Option<&str> = option_env!("SONAR_LICENSE_PUBLIC_KEY");

/// **Development only.** Public half of the dev key pair whose secret is
/// committed in `core/crates/sonar-core/dev/license-dev-secret.key` (anyone
/// reading the repo can mint dev keys). Used when `SONAR_LICENSE_PUBLIC_KEY`
/// is not set at build time. Release builds MUST set that variable
/// (`scripts/release.sh` enforces it).
pub const DEV_PUBLIC_KEY: &str = "ZUD3tqPTmjS5M6GHhTPlsBA20oooXz7cRHn6IIJHrQU";

/// True when this build trusts the committed dev key (not a release key).
pub fn uses_dev_key() -> bool {
    BUILD_PUBLIC_KEY.is_none_or(|k| k.trim().is_empty())
}

/// The public key this build verifies license keys against.
///
/// # Errors
/// [`LicenseError::BadSigningKey`] if the build-time key is malformed.
pub fn builtin_public_key() -> Result<VerifyingKey, LicenseError> {
    let text = match BUILD_PUBLIC_KEY.filter(|k| !k.trim().is_empty()) {
        Some(k) => k,
        None => DEV_PUBLIC_KEY,
    };
    parse_public_key(text)
}

fn key_bytes(text: &str, what: &str) -> Result<[u8; 32], LicenseError> {
    let bytes = b64_decode_lenient(text).map_err(|()| LicenseError::BadSigningKey {
        reason: format!("{what} is not base64"),
    })?;
    <[u8; 32]>::try_from(bytes.as_slice()).map_err(|_| LicenseError::BadSigningKey {
        reason: format!("{what} must be 32 bytes, got {}", bytes.len()),
    })
}

/// Parses a base64 (either alphabet) 32-byte Ed25519 public key.
///
/// # Errors
/// [`LicenseError::BadSigningKey`].
pub fn parse_public_key(text: &str) -> Result<VerifyingKey, LicenseError> {
    VerifyingKey::from_bytes(&key_bytes(text, "public key")?).map_err(|e| {
        LicenseError::BadSigningKey {
            reason: format!("public key: {e}"),
        }
    })
}

/// Parses a base64 32-byte Ed25519 secret key (seed), as written by
/// [`generate_keypair`].
///
/// # Errors
/// [`LicenseError::BadSigningKey`].
pub fn parse_secret_key(text: &str) -> Result<SigningKey, LicenseError> {
    Ok(SigningKey::from_bytes(&key_bytes(text, "secret key")?))
}

/// A freshly generated signing key pair, both halves as base64url text.
#[derive(Debug, Clone)]
pub struct KeyPair {
    /// Secret key (32-byte seed). Keep private; never commit.
    pub secret: String,
    /// Public key to set as `SONAR_LICENSE_PUBLIC_KEY` at build time.
    pub public: String,
}

/// Generates a key pair from OS randomness.
///
/// # Errors
/// [`LicenseError::State`] if the OS cannot provide random bytes.
pub fn generate_keypair() -> Result<KeyPair, LicenseError> {
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).map_err(|e| LicenseError::State {
        reason: format!("no OS randomness: {e}"),
    })?;
    let key = SigningKey::from_bytes(&seed);
    Ok(KeyPair {
        secret: b64_encode(&seed),
        public: b64_encode(key.verifying_key().as_bytes()),
    })
}

/// Signs `info` into a license key string (`SONAR1-...`).
///
/// # Errors
/// [`LicenseError::InvalidIssue`] for empty licensee/id, a subscription
/// without expiry, or an expiry before the issue date.
pub fn issue(secret: &SigningKey, info: &LicenseInfo) -> Result<String, LicenseError> {
    let bad = |reason: &str| LicenseError::InvalidIssue {
        reason: reason.to_owned(),
    };
    if info.licensee.trim().is_empty() || info.license_id.trim().is_empty() {
        return Err(bad("licensee and license id must not be empty"));
    }
    if info.kind == LicenseKind::Subscription && info.expires.is_none() {
        return Err(bad("a subscription needs an expiry date"));
    }
    if info.expires.is_some_and(|e| e < info.issued) {
        return Err(bad("expiry is before the issue date"));
    }
    let payload = Payload {
        v: PAYLOAD_VERSION,
        product: PRODUCT.to_owned(),
        licensee: info.licensee.clone(),
        license_id: info.license_id.clone(),
        issued: info.issued,
        kind: info.kind,
        expires: info.expires,
    };
    let bytes = serde_json::to_vec(&payload).map_err(|e| LicenseError::State {
        reason: format!("payload: {e}"),
    })?;
    let sig = secret.sign(&bytes);
    Ok(format!(
        "{KEY_PREFIX}{}.{}",
        b64_encode(&bytes),
        b64_encode(&sig.to_bytes())
    ))
}

/// The committed DEV signing key (see [`DEV_PUBLIC_KEY`]). Tests only.
#[cfg(any(test, feature = "dev-keys"))]
pub mod dev {
    use super::{parse_secret_key, SigningKey};

    /// Contents of the committed dev secret key file.
    pub const DEV_SECRET_KEY: &str = include_str!("../../dev/license-dev-secret.key");

    /// The dev signing key.
    pub fn signing_key() -> SigningKey {
        parse_secret_key(DEV_SECRET_KEY).expect("committed dev key is valid")
    }
}
