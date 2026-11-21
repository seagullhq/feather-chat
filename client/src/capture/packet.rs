//! VIDEO packet payloads (docs/wire-format.md §4b): fragmenting access units and
//! putting them back together on the viewer.

/// Header: frame u32 | ts u32 | flags u8 | idx u16 | count u16.
pub const HEADER: usize = 13;
/// 1200 byte datagram − 5 byte UDP header − payload header.
pub const MAX_CHUNK: usize = 1200 - 5 - HEADER;

const KEY: u8 = 1;

/// Splits one access unit into VIDEO payloads (without the 5-byte UDP header).
pub fn fragment(frame: u32, ts_ms: u32, key: bool, data: &[u8]) -> Vec<Vec<u8>> {
    let count = data.chunks(MAX_CHUNK).count().max(1) as u16;
    data.chunks(MAX_CHUNK)
        .enumerate()
        .map(|(i, chunk)| {
            let mut p = Vec::with_capacity(HEADER + chunk.len());
            p.extend_from_slice(&frame.to_be_bytes());
            p.extend_from_slice(&ts_ms.to_be_bytes());
            p.push(if key { KEY } else { 0 });
            p.extend_from_slice(&(i as u16).to_be_bytes());
            p.extend_from_slice(&count.to_be_bytes());
            p.extend_from_slice(chunk);
            p
        })
        .collect()
}

/// A fully received access unit.
pub struct Assembled {
    pub ts_ms: u32,
    #[allow(dead_code)] // read by tests; the decoder finds keyframes itself
    pub key: bool,
    pub data: Vec<u8>,
}

struct Partial {
    frame: u32,
    ts_ms: u32,
    key: bool,
    chunks: Vec<Option<Vec<u8>>>,
    got: usize,
}

/// Rebuilds frames from fragments. Only the newest frame is kept: when a newer
/// one starts before the current completes, the current is lost and the
/// stream needs a keyframe before P-frames are usable again.
pub struct Reassembler {
    cur: Option<Partial>,
    last_done: Option<u32>,
    need_key: bool,
}

impl Default for Reassembler {
    fn default() -> Self {
        // A new viewer can only start on a keyframe.
        Self {
            cur: None,
            last_done: None,
            need_key: true,
        }
    }
}

impl Reassembler {
    /// True once per loss: the caller should send SCREEN_KEYREQ (and rate-limit it).
    pub fn take_need_key(&mut self) -> bool {
        std::mem::take(&mut self.need_key)
    }

    /// The decoder choked: drop everything until the next keyframe.
    pub fn request_key(&mut self) {
        self.need_key = true;
    }

    /// Feeds one VIDEO payload; returns a frame when it completes one that is usable.
    pub fn push(&mut self, p: &[u8]) -> Option<Assembled> {
        if p.len() <= HEADER {
            return None;
        }
        let frame = u32::from_be_bytes(p[0..4].try_into().unwrap());
        let ts_ms = u32::from_be_bytes(p[4..8].try_into().unwrap());
        let key = p[8] & KEY != 0;
        let idx = u16::from_be_bytes(p[9..11].try_into().unwrap()) as usize;
        let count = u16::from_be_bytes(p[11..13].try_into().unwrap()) as usize;
        if count == 0 || idx >= count {
            return None;
        }
        let newer = |a: u32, b: u32| (a.wrapping_sub(b) as i32) > 0;
        if self.last_done.is_some_and(|d| !newer(frame, d)) {
            return None; // old or duplicate frame
        }
        if self.cur.as_ref().is_some_and(|c| newer(frame, c.frame)) {
            self.cur = None; // superseded before completion
            self.need_key = true;
        }
        if self.cur.is_none() {
            // A gap in frame ids means a whole frame vanished.
            if self.last_done.is_some_and(|d| frame != d.wrapping_add(1)) {
                self.need_key = true;
            }
            self.cur = Some(Partial {
                frame,
                ts_ms,
                key,
                chunks: vec![None; count],
                got: 0,
            });
        }
        let c = self.cur.as_mut()?;
        if c.frame != frame || c.chunks.len() != count {
            return None; // late fragment of an older frame
        }
        if c.chunks[idx].is_none() {
            c.chunks[idx] = Some(p[HEADER..].to_vec());
            c.got += 1;
        }
        if c.got < count {
            return None;
        }
        let c = self.cur.take()?;
        self.last_done = Some(c.frame);
        if self.need_key && !c.key {
            return None; // P-frame while waiting for a keyframe: undecodable
        }
        if c.key {
            self.need_key = false;
        }
        Some(Assembled {
            ts_ms: c.ts_ms,
            key: c.key,
            data: c.chunks.into_iter().flatten().flatten().collect(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(n: usize, seed: u8) -> Vec<u8> {
        (0..n).map(|i| (i as u8).wrapping_add(seed)).collect()
    }

    #[test]
    fn roundtrip_out_of_order() {
        let d = data(MAX_CHUNK * 5 + 17, 3);
        let mut parts = fragment(7, 1234, true, &d);
        assert_eq!(parts.len(), 6);
        parts.reverse();
        let mut r = Reassembler::default();
        let mut out = None;
        for p in &parts {
            out = r.push(p).or(out);
        }
        let a = out.expect("complete");
        assert!(a.key && a.ts_ms == 1234 && a.data == d);
    }

    #[test]
    fn loss_requires_keyframe() {
        let mut r = Reassembler::default();
        assert!(r.push(&fragment(1, 0, true, &data(100, 0))[0]).is_some());
        assert!(!r.take_need_key()); // keyframe start: nothing lost
        // frame 2 loses a fragment, frame 3 arrives: 2 is dropped, 3 (P) unusable
        let f2 = fragment(2, 0, false, &data(MAX_CHUNK * 2, 1));
        assert!(r.push(&f2[0]).is_none());
        let f3 = fragment(3, 0, false, &data(50, 2));
        assert!(r.push(&f3[0]).is_none());
        assert!(r.take_need_key());
        // a keyframe recovers
        let k = fragment(4, 0, true, &data(50, 3));
        assert!(r.push(&k[0]).is_some());
        assert!(r.push(&fragment(5, 0, false, &data(10, 4))[0]).is_some());
    }

    #[test]
    fn ignores_old_and_garbage() {
        let mut r = Reassembler::default();
        assert!(r.push(&fragment(5, 0, true, &data(10, 0))[0]).is_some());
        assert!(r.push(&fragment(4, 0, true, &data(10, 0))[0]).is_none());
        assert!(r.push(&[1, 2, 3]).is_none());
    }
}
