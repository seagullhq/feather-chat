//! Recovery code (spec §3.3).

use super::Error;
use super::util::{Key, b64, hkdf, labeled, random, sha256};
use zeroize::Zeroizing;

const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// 16 code bytes and 2 check bytes are 144 bits: 29 characters, 1 zero bit.
const CHARS: usize = 29;

pub struct RecoveryCode(Zeroizing<[u8; 16]>);

impl RecoveryCode {
    pub fn generate() -> Result<Self, Error> {
        Ok(Self(Zeroizing::new(random()?)))
    }

    #[cfg(test)]
    pub(super) fn from_bytes(code: [u8; 16]) -> Self {
        Self(Zeroizing::new(code))
    }

    /// For display: groups of five (the last has four), separated by hyphens.
    pub fn display(&self) -> Zeroizing<String> {
        let mut data = Zeroizing::new([0u8; 18]);
        data[..16].copy_from_slice(self.0.as_ref());
        data[16..].copy_from_slice(&sha256(self.0.as_ref())[..2]);
        let mut out = Zeroizing::new(String::with_capacity(CHARS + 5));
        let (mut acc, mut bits, mut n) = (0u32, 0, 0);
        let mut push = |v: u32| {
            if n > 0 && n % 5 == 0 {
                out.push('-');
            }
            out.push(ALPHABET[(v & 31) as usize] as char);
            n += 1;
        };
        for &b in data.iter() {
            acc = (acc << 8) | u32::from(b);
            bits += 8;
            while bits >= 5 {
                bits -= 5;
                push(acc >> bits);
            }
        }
        push(acc << (5 - bits));
        out
    }

    /// Ignores case, spaces and hyphens; reads O as 0 and I, L as 1.
    pub fn parse(input: &str) -> Result<Self, Error> {
        let mut data = Zeroizing::new([0u8; 18]);
        let (mut acc, mut bits, mut chars, mut len) = (0u32, 0, 0, 0);
        for c in input.chars() {
            if c == '-' || c.is_whitespace() {
                continue;
            }
            let c = match c.to_ascii_uppercase() {
                'O' => '0',
                'I' | 'L' => '1',
                c => c,
            };
            let v = ALPHABET
                .iter()
                .position(|&a| a as char == c)
                .ok_or(Error::RecoveryCode)?;
            chars += 1;
            if chars > CHARS {
                return Err(Error::RecoveryCode);
            }
            acc = ((acc << 5) | v as u32) & 0xffff;
            bits += 5;
            if bits >= 8 {
                bits -= 8;
                data[len] = (acc >> bits) as u8;
                len += 1;
            }
        }
        // The single padding bit must be zero.
        if chars != CHARS || acc & 1 != 0 || data[16..] != sha256(&data[..16])[..2] {
            return Err(Error::RecoveryCode);
        }
        let mut code = Zeroizing::new([0u8; 16]);
        code.copy_from_slice(&data[..16]);
        Ok(Self(code))
    }

    /// The `recoveryAuthKey` / `newRecoveryAuthKey` field.
    pub fn auth_key(&self) -> String {
        b64(hkdf(self.0.as_ref(), &labeled("recauth", &[])).as_ref())
    }

    pub(super) fn wrap_key(&self) -> Key {
        hkdf(self.0.as_ref(), &labeled("recwrap", &[]))
    }
}
