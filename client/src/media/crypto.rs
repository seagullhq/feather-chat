//! Per-hop sealing of media datagrams (docs/transport-security.md §2):
//! AES-256-GCM, 13-byte authenticated header `FL | SSRC | counter`.

use aws_lc_rs::aead::{AES_256_GCM, Aad, LessSafeKey, Nonce, UnboundKey};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

use super::wire::{KIND_AUDIO, KIND_PATH, KIND_PING, KIND_SYSAUDIO, KIND_VIDEO};

/// FL, SSRC (u32), counter (u64).
const HEADER: usize = 13;
const TAG: usize = 16;
/// An empty plaintext is rejected.
const MIN_DATAGRAM: usize = HEADER + 1 + TAG;
/// Protocol version bits of FL.
const VERSION: u8 = 1;
const WINDOW: u64 = 1024;

fn nonce(counter: u64) -> Nonce {
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&counter.to_be_bytes());
    Nonce::assume_unique_for_key(n)
}

/// The media keys of one control connection. They outlive the media engine: the
/// uplink counter and the downlink replay window continue across rejoins.
pub struct MediaKeys {
    up: LessSafeKey,
    /// Next uplink counter, shared by every sending thread. Never reused.
    counter: AtomicU64,
    down: LessSafeKey,
    replay: Mutex<ReplayWindow>,
}

impl MediaKeys {
    pub fn new(up: &[u8], down: &[u8]) -> Option<Self> {
        let key = |k| UnboundKey::new(&AES_256_GCM, k).ok().map(LessSafeKey::new);
        Some(Self {
            up: key(up)?,
            counter: AtomicU64::new(0),
            down: key(down)?,
            replay: Mutex::default(),
        })
    }

    /// Seals a plain packet (`FL | SSRC | payload`) into a datagram. `None` if
    /// the packet is too short or the counter is exhausted.
    pub(super) fn seal(&self, packet: &[u8]) -> Option<Vec<u8>> {
        let (head, payload) = packet.split_at_checked(5)?;
        let counter = self
            .counter
            .try_update(Relaxed, Relaxed, |c| c.checked_add(1))
            .ok()?;
        let mut header = [0u8; HEADER];
        header[..5].copy_from_slice(head);
        header[0] = (head[0] & 0x3f) | (VERSION << 6);
        header[5..].copy_from_slice(&counter.to_be_bytes());
        let mut out = Vec::with_capacity(HEADER + payload.len() + TAG);
        out.extend_from_slice(&header);
        out.extend_from_slice(payload);
        let tag = self
            .up
            .seal_in_place_separate_tag(nonce(counter), Aad::from(header), &mut out[HEADER..])
            .ok()?;
        out.extend_from_slice(tag.as_ref());
        Some(out)
    }

    /// Authenticates and decrypts a datagram in place, then applies the replay
    /// check. Returns FL, SSRC and the plaintext.
    pub(super) fn open<'a>(&self, datagram: &'a mut [u8]) -> Option<(u8, u32, &'a [u8])> {
        if datagram.len() < MIN_DATAGRAM {
            return None;
        }
        let (header, sealed) = datagram.split_at_mut(HEADER);
        let header: [u8; HEADER] = (&*header).try_into().ok()?;
        let flags = header[0];
        if flags >> 6 != VERSION
            || !matches!(
                flags & 0x1f,
                KIND_AUDIO | KIND_PING | KIND_VIDEO | KIND_SYSAUDIO | KIND_PATH
            )
        {
            return None;
        }
        let ssrc = u32::from_be_bytes(header[1..5].try_into().ok()?);
        let counter = u64::from_be_bytes(header[5..].try_into().ok()?);
        let plain = self
            .down
            .open_in_place(nonce(counter), Aad::from(header), sealed)
            .ok()?;
        // Only an authenticated counter may move the window.
        if !self.replay.lock().ok()?.accept(counter) {
            return None;
        }
        Some((flags, ssrc, plain))
    }
}

/// The last `WINDOW` counters below the highest accepted one.
#[derive(Default)]
struct ReplayWindow {
    highest: Option<u64>,
    /// One bit per counter, indexed modulo `WINDOW`.
    seen: [u64; WINDOW as usize / 64],
}

impl ReplayWindow {
    /// Records `counter`; false if it was already seen or is too old.
    fn accept(&mut self, counter: u64) -> bool {
        match self.highest {
            Some(h) if counter <= h => {
                if h - counter >= WINDOW {
                    return false;
                }
            }
            Some(h) => {
                // Forget the slots the window slides over.
                if counter - h >= WINDOW {
                    self.seen.fill(0);
                } else {
                    for c in h + 1..=counter {
                        let (word, bit) = Self::slot(c);
                        self.seen[word] &= !bit;
                    }
                }
                self.highest = Some(counter);
            }
            None => self.highest = Some(counter),
        }
        let (word, bit) = Self::slot(counter);
        let fresh = self.seen[word] & bit == 0;
        self.seen[word] |= bit;
        fresh
    }

    fn slot(counter: u64) -> (usize, u64) {
        let i = (counter % WINDOW) as usize;
        (i / 64, 1 << (i % 64))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// docs/transport-security.md §3.
    const VECTOR: &str = "401122334400000000000000078608726b7f722c7db028f2e43f9e807e9cce80a420a711";

    /// Same key both ways, so a client can open what it sealed.
    fn keys() -> MediaKeys {
        let key: Vec<u8> = (0..32).collect();
        MediaKeys::new(&key, &key).unwrap()
    }

    fn packet() -> Vec<u8> {
        let mut p = vec![KIND_AUDIO, 0x11, 0x22, 0x33, 0x44];
        p.extend(b"feather");
        p
    }

    fn counter(datagram: &[u8]) -> u64 {
        u64::from_be_bytes(datagram[5..13].try_into().unwrap())
    }

    #[test]
    fn matches_the_spec_test_vector() {
        let k = keys();
        for _ in 0..7 {
            k.seal(&packet()).unwrap();
        }
        let mut d = k.seal(&packet()).unwrap();
        let hex: String = d.iter().map(|b| format!("{b:02x}")).collect();
        assert_eq!(hex, VECTOR);
        let (flags, ssrc, plain) = k.open(&mut d).unwrap();
        assert_eq!(
            (flags, ssrc, plain),
            (0x40, 0x11223344, b"feather".as_slice())
        );
    }

    #[test]
    fn rejects_tampering_replay_and_short_datagrams() {
        let k = keys();
        let good = k.seal(&packet()).unwrap();
        // Every byte is authenticated: header (AAD), ciphertext and tag.
        for i in 0..good.len() {
            let mut bad = good.clone();
            bad[i] ^= 1;
            assert!(k.open(&mut bad).is_none(), "byte {i}");
        }
        assert!(k.open(&mut good[..good.len() - 1].to_vec()).is_none());
        assert!(k.open(&mut [0x40; 29]).is_none());
        let empty = k.seal(&packet()[..5]).unwrap();
        assert!(k.open(&mut empty.clone()).is_none());
        // The failures above did not consume the counter.
        assert!(k.open(&mut good.clone()).is_some());
        assert!(k.open(&mut good.clone()).is_none(), "replay");
        assert!(k.seal(&[0; 4]).is_none());
    }

    #[test]
    fn counter_is_shared_across_threads_and_never_wraps() {
        let k = keys();
        let mut all: Vec<u64> = std::thread::scope(|s| {
            let threads: Vec<_> = (0..4)
                .map(|_| {
                    s.spawn(|| {
                        (0..500)
                            .map(|_| counter(&k.seal(&packet()).unwrap()))
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            threads
                .into_iter()
                .flat_map(|t| t.join().unwrap())
                .collect()
        });
        all.sort_unstable();
        assert_eq!(all, (0..2000).collect::<Vec<_>>());
        k.counter.store(u64::MAX, Relaxed);
        assert!(k.seal(&packet()).is_none());
        assert!(k.seal(&packet()).is_none());
    }

    #[test]
    fn replay_window() {
        let mut w = ReplayWindow::default();
        assert!(w.accept(0));
        assert!(!w.accept(0));
        assert!(w.accept(5));
        assert!(w.accept(3)); // reordered
        assert!(!w.accept(3));
        assert!(w.accept(2000));
        assert!(!w.accept(2000 - 1024), "older than highest - 1023");
        assert!(w.accept(2000 - 1023));
        assert!(!w.accept(2000 - 1023));
        assert!(w.accept(2001));
        assert!(!w.accept(977), "slid out of the window");
        // Sliding reuses slots (10 and 1034 share one): old marks must be gone.
        let mut r = ReplayWindow::default();
        assert!(r.accept(10));
        assert!(r.accept(1000));
        assert!(r.accept(1100));
        assert!(r.accept(1034));
        assert!(!r.accept(1034));
        assert!(!r.accept(1000));
        // A jump past the whole window clears it.
        assert!(w.accept(1_000_000));
        assert!(w.accept(1_000_000 - 1023));
        assert!(w.accept(u64::MAX));
        assert!(!w.accept(u64::MAX));
    }
}
