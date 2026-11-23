# Client audio pipeline

Design notes for the in-client media path. Server-side forwarding is in
[server-design.md](server-design.md); protocol rationale in
[protocol.md](protocol.md); datagram layout in [wire-format.md](wire-format.md).

```
mic ──► cpal ──► ring ──► Opus (20 ms) ──► UDP AUDIO
                                              ▲ server forwards per SSRC
desktop output ──► ring ──► Opus (20 ms) ──► UDP SYSAUDIO ──► watchers only
                                              │
UDP ──► per-speaker jitter buffer ──► Opus ──► mixer ──► cpal speakers
```

## Capture

- Format is 48 kHz mono i16; encoder is Opus, 20 ms = 960 samples.
- The mic stream comes from cpal; a device rate other than 48 kHz is linearly
  resampled. Device callbacks only push into lock-free rings.
- Desktop audio is a **separate opt-in toggle**, not part of starting a share.
  Video and audio can be started/stopped independently; stopping the share
  always stops audio. It uses the presenter's SSRC and a separate Opus sequence.
  - **Windows**: cpal loopback input stream on the selected output (render)
    device; cpal sets `AUDCLNT_STREAMFLAGS_LOOPBACK`. WASAPI's
    `DATA_DISCONTINUITY` when playback resumes after silence is a benign
    `Xrun` and is ignored; real device/stream errors still reset the toggle.
  - **macOS**: a second, audio-only ScreenCaptureKit stream
    (`with_captures_audio(true)`, `SCStreamOutputType::Audio`), downmixed to mono;
    the video stream captures video only.
  - **Linux**: the PulseAudio monitor source (`Monitor of ...` / `*.monitor`)
    of the selected output device, via cpal.
- Each encoded frame is one datagram: AUDIO (kind 0) for the mic, SYSAUDIO
  (kind 3) for desktop audio. The server forwards SYSAUDIO only to sessions
  subscribed with `SCREEN_WATCH`. Turning audio off sends one terminator frame.
- Capture setup or runtime failures are reported to the UI and reset the
  toggle; they never silently leave a live-looking control. While the toggle
  is on, the call bar shows an underline meter of the level actually reaching
  the Opus encoder, so silence and capture failure look different.

## Playback

- Per-remote-SSRC jitter buffer, primed to 2 frames (up to 6 under jitter),
  driven by the 20 ms mix loop. Late packets are dropped; gaps use Opus PLC.
- The **T flag** ends the talkspurt, resets PLC and stops the level indicator.
- Mic and desktop audio from the same SSRC use separate jitter buffers. Only
  the mic drives the speaking ring; both are summed into the output mix.
- Deafening keeps the speakers silent for both kinds.

## Threading

- The audio callback does not allocate, lock, or do blocking IO — it only
  pushes into a lock-free ring buffer.
- The worker thread does ping, receive, encode, decode and mix on a fast loop;
  video decode and desktop-audio capture each own a separate thread.
- No GUI objects are touched off the UI thread.
