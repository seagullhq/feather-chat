//! Worker thread: ping, receive, encode, decode and mix on a fast loop.

use opus_rs::{Application, OpusEncoder};
use std::collections::HashMap;
use std::net::UdpSocket;
use std::sync::Arc;
use std::sync::atomic::Ordering::Relaxed;
use std::time::{Duration, Instant};

use super::peer::Peer;
use super::prefs::PREFS;
use super::shared::Shared;
use super::wire::{KIND_AUDIO, KIND_PING, TERMINATOR, audio_packet, header, rms};
use super::{FRAME, OUT_RING, PEER_TIMEOUT, RATE};

pub(super) fn worker(
    shared: Arc<Shared>,
    sock: UdpSocket,
    ssrc: u32,
    mut cap: rtrb::Consumer<i16>,
    mut out: rtrb::Producer<f32>,
) {
    let mut enc = match OpusEncoder::new(RATE as i32, 1, Application::Voip) {
        Ok(e) => e,
        Err(e) => return eprintln!("opus encoder: {e}"),
    };
    enc.bitrate_bps = 32_000;
    let mut peers: HashMap<u32, Peer> = HashMap::new();
    let (mut seq, mut was_sending, mut echoed) = (0u32, false, false);
    let (mut pcm, mut have) = ([0i16; FRAME], 0usize);
    let (mut coded, mut rx) = ([0u8; 400], [0u8; 2048]);
    let mut mic_level = 0f32;
    let t0 = Instant::now();
    let (mut next_ping, mut next_mix) = (t0, t0);

    while !shared.stop.load(Relaxed) {
        let now = Instant::now();

        // Keepalive + connectivity check: fast until the first echo, then 1 Hz.
        if now >= next_ping {
            let mut p = header(KIND_PING, ssrc);
            p.extend((now - t0).as_micros().to_be_bytes()[8..].iter());
            let _ = sock.send(&p);
            next_ping = now + Duration::from_millis(if echoed { 1000 } else { 200 });
        }

        // Receive everything queued.
        while let Ok(n) = sock.recv(&mut rx) {
            if n < 5 || rx[0] >> 6 != 0 {
                continue;
            }
            let from = u32::from_be_bytes([rx[1], rx[2], rx[3], rx[4]]);
            match rx[0] & 0x1f {
                KIND_PING => {
                    echoed = true;
                    // The server echoes our datagram: bytes 5..13 are the send time.
                    if n >= 13 {
                        let sent = u64::from_be_bytes(rx[5..13].try_into().unwrap());
                        let rtt = (t0.elapsed().as_micros() as u64).saturating_sub(sent);
                        shared
                            .rtt_us
                            .store(rtt.clamp(1, u32::MAX as u64) as u32, Relaxed);
                    }
                }
                KIND_AUDIO if n > 9 && from != ssrc => {
                    let seq = u32::from_be_bytes([rx[5], rx[6], rx[7], rx[8]]);
                    peers.entry(from).or_insert_with(Peer::new).insert(
                        seq,
                        rx[9..n].to_vec(),
                        rx[0] & TERMINATOR != 0,
                    );
                }
                _ => {}
            }
        }

        // Capture -> Opus -> UDP, one 20 ms frame at a time.
        while let Ok(s) = cap.pop() {
            pcm[have] = s;
            have += 1;
            if have < FRAME {
                continue;
            }
            have = 0;
            let silent = shared.muted.load(Relaxed) || shared.deafened.load(Relaxed);
            let level = rms(pcm.iter().map(|&s| s as f32 / 32768.), FRAME);
            mic_level = if silent {
                0.
            } else {
                mic_level * 0.55 + level * 0.45
            };
            shared.mic.store(mic_level.to_bits(), Relaxed);
            // Muting ends the talkspurt with one terminator frame, then stops.
            let term = silent && was_sending;
            if silent && !term {
                continue;
            }
            if term {
                pcm.fill(0);
            }
            was_sending = !silent;
            if let Ok(len) = enc.encode_i16(&pcm, FRAME, &mut coded) {
                let p = audio_packet(ssrc, seq, term, &coded[..len]);
                let _ = sock.send(&p);
                seq = seq.wrapping_add(1);
            }
        }

        // Mix every 20 ms of wall clock.
        if now >= next_mix {
            next_mix += Duration::from_millis(20);
            if next_mix < now {
                next_mix = now + Duration::from_millis(20); // fell behind: don't burst
            }
            mix(&shared, &mut peers, &mut out);
        }
        // Poll fast: every ms of sleep is added to packet latency both ways.
        std::thread::sleep(Duration::from_millis(1));
    }
}

fn mix(shared: &Shared, peers: &mut HashMap<u32, Peer>, out: &mut rtrb::Producer<f32>) {
    peers.retain(|_, p| p.last.elapsed() < PEER_TIMEOUT);
    let mut acc = [0f32; FRAME];
    let mut levels = HashMap::new();
    for (ssrc, p) in peers.iter_mut() {
        p.step(&mut acc);
        levels.insert(*ssrc, p.level);
    }
    if let Ok(mut l) = shared.peers.lock() {
        *l = levels;
    }
    // Deafened: speakers stay silent. Ring already holds enough: skip this
    // frame so output delay can't build up when the device clock is slower.
    if shared.deafened.load(Relaxed) || OUT_RING - out.slots() > 4 * FRAME {
        return;
    }
    let v = PREFS.volume.load(Relaxed) as f32 / 100.;
    let gain = v * v;
    for s in acc {
        let _ = out.push((s * gain).clamp(-1., 1.));
    }
}
