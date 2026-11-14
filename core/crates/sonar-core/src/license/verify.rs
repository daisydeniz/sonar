//! License key format, the [`LicenseVerifier`] trait and the offline
//! Ed25519 implementation [`SignedKeyVerifier`].
//!
//! Key text: `SONAR1-<b64url(payload json)>.<b64url(ed25519 signature)>`
//! (base64url, no padding; the signature covers the payload bytes exactly as
//! they appear in the key). Whitespace anywhere in a pasted key is ignored.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};

use super::{Date, LicenseError};

/// Prefix of every key (also the key format generation).
pub const KEY_PREFIX: &str = "SONAR1-";
/// Payload `product` value of Sonar keys.
pub const PRODUCT: &str = "sonar";
/// The only payload version this build understands.
pub const PAYLOAD_VERSION: u32 = 1;
/// Keys longer than this (after removing whitespace) are rejected unread.
const MAX_KEY_LEN: usize = 8 * 1024;

/// Perpetual or time-limited license.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LicenseKind {
    /// Valid forever (`expires` is normally absent).
    Perpetual,
    /// Valid through the `expires` date.
    Subscription,
}

impl LicenseKind {
    /// `"perpetual"` or `"subscription"`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Perpetual => "perpetual",
            Self::Subscription => "subscription",
        }
    }
}

impl std::str::FromStr for LicenseKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "perpetual" => Ok(Self::Perpetual),
            "subscription" => Ok(Self::Subscription),
            _ => Err(format!("'{s}' is not perpetual or subscription")),
        }
    }
}

/// What a valid key says about the license.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LicenseInfo {
    /// Name or e-mail of the licensee.
    pub licensee: String,
    /// Opaque id from the store or payment provider (for support lookups).
    pub license_id: String,
    /// Day the key was issued.
    pub issued: Date,
    /// Perpetual or subscription.
    pub kind: LicenseKind,
    /// Last valid day, if limited.
    pub expires: Option<Date>,
}

impl LicenseInfo {
    /// True if `today` is after the last valid day.
    pub fn is_expired_on(&self, today: Date) -> bool {
        self.expires.is_some_and(|e| today > e)
    }
}

/// Signed JSON payload of a key (wire format).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Payload {
    pub v: u32,
    pub product: String,
    pub licensee: String,
    pub license_id: String,
    pub issued: Date,
    pub kind: LicenseKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<Date>,
}

/// Anything that can turn a license key into a [`LicenseInfo`].
///
/// The app never depends on a payment provider. A store (Paddle, Lemon
/// Squeezy, Gumroad, ...) only has to *fulfil* an order by issuing a Sonar
/// signed key: its webhook calls `sonar license issue` (or a small server
/// using [`super::issue`], same format) and e-mails the key to the buyer.
/// Verification stays offline in [`SignedKeyVerifier`]. A verifier that asks
/// a provider's API (online activation, seat limits) could implement this
/// trait later without touching the callers.
pub trait LicenseVerifier {
    /// Checks authenticity, product and format version, but **not** the
    /// expiry date. Used to describe an expired license.
    ///
    /// # Errors
    /// [`LicenseError::Malformed`], [`LicenseError::BadSignature`],
    /// [`LicenseError::WrongProduct`], [`LicenseError::UnsupportedVersion`].
    fn inspect(&self, key: &str) -> Result<LicenseInfo, LicenseError>;

    /// [`inspect`](Self::inspect) plus the expiry check against `today`.
    ///
    /// # Errors
    /// Those of `inspect`, and [`LicenseError::Expired`].
    fn verify(&self, key: &str, today: Date) -> Result<LicenseInfo, LicenseError> {
        let info = self.inspect(key)?;
        match info.expires {
            Some(date) if info.is_expired_on(today) => Err(LicenseError::Expired { date }),
            _ => Ok(info),
        }
    }
}

/// Offline verifier: the key carries an Ed25519 signature by Sonar's license
/// key; no network, no server.
#[derive(Debug, Clone)]
pub struct SignedKeyVerifier {
    public_key: VerifyingKey,
}

impl SignedKeyVerifier {
    /// Verifier for keys signed by the secret half of `public_key`.
    pub fn new(public_key: VerifyingKey) -> Self {
        Self { public_key }
    }

    /// Verifier for the public key this build ships with (see
    /// [`super::builtin_public_key`]).
    ///
    /// # Errors
    /// [`LicenseError::BadSigningKey`] if `SONAR_LICENSE_PUBLIC_KEY` was set
    /// at build time to something that is not a 32-byte Ed25519 key. Every
    /// status call then errors and the app fails open (not gated), so
    /// `scripts/release.sh` validates the key before building.
    pub fn builtin() -> Result<Self, LicenseError> {
        super::builtin_public_key().map(Self::new)
    }
}

/// Removes all whitespace (pasted keys are often wrapped or padded).
pub fn normalize_key(key: &str) -> String {
    key.chars().filter(|c| !c.is_whitespace()).collect()
}

impl LicenseVerifier for SignedKeyVerifier {
    fn inspect(&self, key: &str) -> Result<LicenseInfo, LicenseError> {
        let key = normalize_key(key);
        if key.len() > MAX_KEY_LEN {
            return Err(LicenseError::malformed("too long"));
        }
        let body = key
            .strip_prefix(KEY_PREFIX)
            .ok_or_else(|| LicenseError::malformed("missing SONAR1- prefix"))?;
        let (payload_b64, sig_b64) = body
            .split_once('.')
            .ok_or_else(|| LicenseError::malformed("missing signature"))?;
        let payload_bytes = URL_SAFE_NO_PAD
            .decode(payload_b64)
            .map_err(|_| LicenseError::malformed("payload is not base64"))?;
        let sig_bytes = URL_SAFE_NO_PAD
            .decode(sig_b64)
            .map_err(|_| LicenseError::malformed("signature is not base64"))?;
        let sig = Signature::from_slice(&sig_bytes)
            .map_err(|_| LicenseError::malformed("signature has the wrong length"))?;
        // Authenticity first: nothing in the payload is trusted before this.
        self.public_key
            .verify_strict(&payload_bytes, &sig)
            .map_err(|_| LicenseError::BadSignature)?;

        // Version before the full parse: a newer format may differ in shape.
        let value: serde_json::Value = serde_json::from_slice(&payload_bytes)
            .map_err(|_| LicenseError::malformed("payload is not JSON"))?;
        let v = value.get("v").and_then(serde_json::Value::as_u64);
        if v != Some(u64::from(PAYLOAD_VERSION)) {
            let found = v.map_or(0, |v| u32::try_from(v).unwrap_or(u32::MAX));
            return Err(LicenseError::UnsupportedVersion { found });
        }
        let payload: Payload = serde_json::from_value(value)
            .map_err(|e| LicenseError::malformed(&format!("payload fields: {e}")))?;
        if payload.product != PRODUCT {
            return Err(LicenseError::WrongProduct {
                found: payload.product,
            });
        }
        Ok(LicenseInfo {
            licensee: payload.licensee,
            license_id: payload.license_id,
            issued: payload.issued,
            kind: payload.kind,
            expires: payload.expires,
        })
    }
}

/// Decodes base64 in either alphabet, with or without padding.
pub(crate) fn b64_decode_lenient(s: &str) -> Result<Vec<u8>, ()> {
    let s: String = s
        .trim()
        .trim_end_matches('=')
        .chars()
        .map(|c| match c {
            '+' => '-',
            '/' => '_',
            c => c,
        })
        .collect();
    URL_SAFE_NO_PAD.decode(s).map_err(|_| ())
}

/// base64url (no padding) of `bytes`.
pub(crate) fn b64_encode(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}
