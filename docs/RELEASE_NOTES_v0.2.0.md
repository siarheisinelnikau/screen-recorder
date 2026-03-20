# ScreenRecorder 0.2.0 — local release draft

Adds optional audio capture and compact MP4 export to the 0.1 implementation. The comparison baseline is the local 0.1 milestone `fb2196a`; no preceding published release or new remote release was created in this session.

## Added

- Opt into the default microphone, default system-output mix, or both when starting a recording. WASAPI clients stop for pause and finish with the recording; failures retain completed data.
- Separate recoverable PCM audio journals with shared video/audio clock mapping, native-format conversion, pause/trim synchronization, gain controls and a stereo mix.
- Export H.264 video with optional AAC audio directly through Windows Media Foundation, without FFmpeg. Export observes cancellation, preserves existing destinations and atomically publishes finalized files.
- Synthetic native encoder/decoder tests and an independent MP4 container inspector. No capture endpoint, GUI or audio playback is involved.

## Improved

- Recording setup now separates source, sound and saving. Filename/frame-rate overrides, manual zoom and gain adjustments expand only when needed; MP4 is the default export.
- Shared palette/type/spacing roles and a persistent starting/recording/paused/finalizing indicator. Pause is enabled after the first completed frame; Stop remains available during preparation.
- A finite offscreen mode renders the actual UI to PNG without windows/devices/network. The resulting review fixed hidden export/error controls, a truncated settings path and a missing Start glyph. Primary actions remain visible in a footer at the minimum viewport. Normal OpenGL/idle behavior still requires a separate runtime gate.
- Integer MP4 sample scheduling avoids an extra zero-duration frame at floating-point trim boundaries. Finalized MP4 files are synced after the native sink releases its stream.

## Compatibility and known limits

- Package: `ScreenRecorder-v0.2.0.exe`, Windows x64, static CRT, plus checksums/license notices. Windows Media Foundation H.264/AAC is required; Windows N needs Media Feature Pack. The existing UI still uses OpenGL and has not passed a runtime rendering/performance gate.
- Existing video-only projects/settings/edit files remain readable. Keep new `.mic.saudio` and `.system.saudio` files with their `.srec`; the 0.1 application cannot export those audio tracks.
- Audio captures default endpoints, with system loopback covering all applications on that output. No endpoint picker, per-app loopback, monitoring, audio preview or automatic device switching. AVI remains silent. Odd MP4 dimensions pad to even pixels; AAC can add priming/padding.
- Synthetic encoding/decoding, code checks, packaging and checked application-generated CPU PNGs are the available evidence. Real microphone/system capture, long-session A/V sync, device removal/privacy denial, normal GUI/OpenGL/idle behavior and update replacement need manual validation. This is an unpublished candidate, not a hardware-validated release.
