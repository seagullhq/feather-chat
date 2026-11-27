//! Certificate pinning (docs/transport-security.md §1.2): the leaf fingerprint
//! is pinned per host in `known_servers.json`. No CA store, names or expiry.

use aws_lc_rs::digest::{SHA256, digest};
#[cfg(not(test))]
use directories::ProjectDirs;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::crypto::{WebPkiSupportedAlgorithms, verify_tls12_signature, verify_tls13_signature};
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::{CertificateError, DigitallySignedStruct, SignatureScheme};
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const FILE: &str = "known_servers.json";

/// A server whose certificate the user has not accepted: the handshake was
/// aborted before any application byte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Untrusted {
    pub host: String,
    /// What the server presented.
    pub fingerprint: String,
    /// The previously trusted value; `None` for an unknown host.
    pub pinned: Option<String>,
}

/// SHA-256 over the DER certificate, lowercase hex.
pub(super) fn fingerprint(der: &[u8]) -> String {
    digest(&SHA256, der)
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Replaces chain, name and expiry validation with the pin. The handshake
/// signature is still verified by rustls.
#[derive(Debug)]
pub(super) struct PinVerifier {
    pin: Option<String>,
    /// Fingerprint that did not match the pin, for the trust prompt.
    rejected: Mutex<Option<String>>,
    algorithms: WebPkiSupportedAlgorithms,
}

impl PinVerifier {
    pub(super) fn new(pin: Option<String>, algorithms: WebPkiSupportedAlgorithms) -> Self {
        Self {
            pin,
            rejected: Mutex::new(None),
            algorithms,
        }
    }

    /// Set when the handshake failed because of the pin.
    pub(super) fn untrusted(&self, host: &str) -> Option<Untrusted> {
        let fingerprint = self.rejected.lock().ok()?.clone()?;
        Some(Untrusted {
            host: host.to_string(),
            fingerprint,
            pinned: self.pin.clone(),
        })
    }
}

impl ServerCertVerifier for PinVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &CertificateDer<'_>,
        _intermediates: &[CertificateDer<'_>],
        _server_name: &ServerName<'_>,
        _ocsp_response: &[u8],
        _now: UnixTime,
    ) -> Result<ServerCertVerified, rustls::Error> {
        let seen = fingerprint(end_entity);
        if self.pin.as_deref() == Some(seen.as_str()) {
            return Ok(ServerCertVerified::assertion());
        }
        if let Ok(mut r) = self.rejected.lock() {
            *r = Some(seen);
        }
        Err(CertificateError::ApplicationVerificationFailure.into())
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls12_signature(message, cert, dss, &self.algorithms)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> Result<HandshakeSignatureValid, rustls::Error> {
        verify_tls13_signature(message, cert, dss, &self.algorithms)
    }

    fn supported_verify_schemes(&self) -> Vec<SignatureScheme> {
        self.algorithms.supported_schemes()
    }
}

/// `known_servers.json` in the application config directory.
#[cfg(not(test))]
fn path() -> Result<PathBuf, String> {
    ProjectDirs::from("dev", "Seagullcode", "FeatherChat")
        .map(|d| d.config_dir().join(FILE))
        .ok_or_else(|| "Could not find the config directory.".into())
}

/// Tests never touch the user's pins.
#[cfg(test)]
pub(super) fn path() -> Result<PathBuf, String> {
    Ok(std::env::temp_dir()
        .join(format!("feather-chat-test-{}", std::process::id()))
        .join(FILE))
}

/// A missing file means no pins. Anything else that cannot be understood is an
/// error: treating it as empty would hide a changed fingerprint.
fn load(path: &Path) -> Result<BTreeMap<String, String>, String> {
    let bad = |e: String| {
        format!(
            "Cannot read the trusted servers file {}: {e}. Fix or delete it.",
            path.display()
        )
    };
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(e) => return Err(bad(e.to_string())),
    };
    let pins: BTreeMap<String, String> =
        serde_json::from_str(&text).map_err(|e| bad(e.to_string()))?;
    let valid =
        |p: &String| p.len() == 64 && p.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    if !pins.values().all(valid) {
        return Err(bad("invalid fingerprint".into()));
    }
    Ok(pins)
}

/// Write to a temporary file, then rename over the old one.
fn save(path: &Path, host: &str, fingerprint: &str) -> Result<(), String> {
    let mut pins = load(path)?;
    pins.insert(host.to_lowercase(), fingerprint.to_string());
    let write = || -> io::Result<()> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension(format!("{}.tmp", std::process::id()));
        let mut f = fs::File::create(&tmp)?;
        f.write_all(serde_json::to_string_pretty(&pins)?.as_bytes())?;
        f.sync_all()?;
        drop(f);
        fs::rename(&tmp, path)
    };
    write().map_err(|e| format!("Could not save the trusted server: {e}"))
}

/// The pinned fingerprint of `host`, if any.
pub(super) fn pinned(host: &str) -> Result<Option<String>, String> {
    Ok(load(&path()?)?.remove(&host.to_lowercase()))
}

/// The user accepted `fingerprint` for `host`.
pub fn trust(host: &str, fingerprint: &str) -> Result<(), String> {
    save(&path()?, host, fingerprint)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verifier(pin: Option<String>) -> PinVerifier {
        let provider = rustls::crypto::aws_lc_rs::default_provider();
        PinVerifier::new(pin, provider.signature_verification_algorithms)
    }

    fn check(v: &PinVerifier, cert: &[u8]) -> bool {
        v.verify_server_cert(
            &CertificateDer::from(cert.to_vec()),
            &[],
            &ServerName::try_from("localhost").unwrap(),
            &[],
            UnixTime::now(),
        )
        .is_ok()
    }

    #[test]
    fn fingerprint_is_lowercase_sha256() {
        assert_eq!(
            fingerprint(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn verifier_accepts_only_the_pinned_certificate() {
        let ok = verifier(Some(fingerprint(b"cert")));
        assert!(check(&ok, b"cert"));
        assert_eq!(ok.untrusted("h"), None);

        let changed = verifier(Some(fingerprint(b"old")));
        assert!(!check(&changed, b"cert"));
        let u = changed.untrusted("h").unwrap();
        assert_eq!(u.fingerprint, fingerprint(b"cert"));
        assert_eq!(u.pinned, Some(fingerprint(b"old")));

        let unknown = verifier(None);
        assert!(!check(&unknown, b"cert"));
        assert_eq!(unknown.untrusted("h").unwrap().pinned, None);
    }

    #[test]
    fn pin_file_round_trip_and_corruption() {
        let dir = std::env::temp_dir().join(format!("feather-chat-pins-{}", std::process::id()));
        let file = dir.join(FILE);
        let _ = fs::remove_dir_all(&dir);
        assert!(load(&file).unwrap().is_empty());

        let (a, b) = (fingerprint(b"a"), fingerprint(b"b"));
        save(&file, "Example.ORG", &a).unwrap();
        save(&file, "other", &a).unwrap();
        save(&file, "example.org", &b).unwrap();
        let pins = load(&file).unwrap();
        assert_eq!(pins.len(), 2);
        assert_eq!(pins["example.org"], b);

        for corrupt in ["", "not json", "[]", r#"{"h":1}"#, r#"{"h":"abc"}"#] {
            fs::write(&file, corrupt).unwrap();
            assert!(load(&file).is_err(), "{corrupt:?}");
            // Never overwritten behind the user's back either.
            assert!(save(&file, "h", &a).is_err());
            assert_eq!(fs::read_to_string(&file).unwrap(), corrupt);
        }
        fs::remove_dir_all(&dir).unwrap();
    }
}
