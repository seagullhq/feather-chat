//! Encoding, HKDF, AEAD boxes and randomness (spec §2).

use super::Error;
use base64::{Engine as _, engine::general_purpose::STANDARD};
use chacha20poly1305::aead::{Aead, Payload};
use chacha20poly1305::{KeyInit, XChaCha20Poly1305};
use hkdf::Hkdf;
use serde_json::Value;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub(super) type Key = Zeroizing<[u8; 32]>;

/// `FC1:<name>` followed by a zero byte, then `parts`.
pub(super) fn labeled(name: &str, parts: &[&[u8]]) -> Vec<u8> {
    let mut out = format!("FC1:{name}\0").into_bytes();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}

pub(super) fn hkdf(ikm: &[u8], info: &[u8]) -> Key {
    let mut out = Key::default();
    Hkdf::<Sha256>::new(None, ikm)
        .expand(info, out.as_mut())
        .expect("32 bytes is a valid HKDF length");
    out
}

pub(super) fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub(super) fn random<const N: usize>() -> Result<[u8; N], Error> {
    let mut out = [0; N];
    getrandom::fill(&mut out).map_err(|e| Error::Store(format!("No random numbers: {e}")))?;
    Ok(out)
}

/// `nonce (24) ‖ ciphertext ‖ tag (16)`.
pub(super) fn seal_box(key: &[u8; 32], plain: &[u8], aad: &[u8]) -> Result<Vec<u8>, Error> {
    let nonce = random::<24>()?;
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    let sealed = cipher
        .encrypt(&nonce.into(), Payload { msg: plain, aad })
        .map_err(|_| Error::Crypto)?;
    Ok([&nonce[..], &sealed].concat())
}

pub(super) fn open_box(
    key: &[u8; 32],
    boxed: &[u8],
    aad: &[u8],
) -> Result<Zeroizing<Vec<u8>>, Error> {
    let (nonce, sealed) = boxed.split_first_chunk::<24>().ok_or(Error::Crypto)?;
    let cipher = XChaCha20Poly1305::new(&(*key).into());
    cipher
        .decrypt(&(*nonce).into(), Payload { msg: sealed, aad })
        .map(Zeroizing::new)
        .map_err(|_| Error::Crypto)
}

pub(super) fn b64(data: &[u8]) -> String {
    STANDARD.encode(data)
}

pub(super) fn bytes(v: &Value, field: &'static str) -> Result<Vec<u8>, Error> {
    v[field]
        .as_str()
        .and_then(|s| STANDARD.decode(s).ok())
        .ok_or(Error::Malformed(field))
}

pub(super) fn array<const N: usize>(v: &Value, field: &'static str) -> Result<[u8; N], Error> {
    bytes(v, field)?
        .try_into()
        .map_err(|_| Error::Malformed(field))
}

pub(super) fn uint(v: &Value, field: &'static str) -> Result<u64, Error> {
    v[field].as_u64().ok_or(Error::Malformed(field))
}

pub(super) fn uint32(v: &Value, field: &'static str) -> Result<u32, Error> {
    u32::try_from(uint(v, field)?).map_err(|_| Error::Malformed(field))
}

pub(super) fn list<'a>(v: &'a Value, field: &'static str) -> Result<&'a [Value], Error> {
    v[field]
        .as_array()
        .map(Vec::as_slice)
        .ok_or(Error::Malformed(field))
}
