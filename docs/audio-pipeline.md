# Client audio pipeline (Qt side)

Design notes for the in-client media path. Server-side forwarding is in
[server-design.md](server-design.md); protocol rationale in
[protocol.md](protocol.md).

```
mic ─► QAudioSource ─► ring buffer ─► Opus encode (20 ms) ─► QUdpSocket -► server
                                        ▲ server received AUDIO per-SSRC ─► jitter buffer
                                        │ per SSRC ─► Opus decode ─► mixer ─► QAudioSink
```

## Capture

- `QAudioSource` with the callback API where the Qt version allows
  (6.11+), `QIODevice` fallback otherwise — the callback is the only
  low-latency interface Qt offers, and its thread is must-not-block.
- Format: Int16 mono at 48 kHz. Encoder:
  `opus_encoder_create(48000, 1, OPUS_APPLICATION_VOIP)`. Frame = 20 ms =
  960 samples.
- Each encoded frame becomes one UDP datagram with header + seq per
  [wire-format.md](wire-format.md).

## Playback

- Per-remote-SSRC jitter buffer: sorted deque sized 3–6 frames (60–120 ms
  typically); driven by `QAudioSink`'s pull. Packets arriving after their
  playout point are dropped (late == lost).
- Packet Loss Concealment: gaps passed to `opus_decode(NULL, ...)`.
- The **T flag** resets both the decoder and the speaking indicator on the
  next packet, without timeout heuristics.
- Mixer: sum PCM of active SSRCs into the output buffer, applying per-source
  volume limiting.

## Threading

- The audio callback does not allocate, lock, or do blocking IO — it only
  pushes into a lock-free ring buffer.
- UDP receive lives on a dedicated `QThread` or event loop; no GUI objects
  off the GUI thread.

## Out of scope

DTX and timestamps are excluded (see [protocol.md](protocol.md));
encryption is a later roadmap item documented in
[security-model.md](security-model.md).
