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
- A finite offscreen mode renders the actual UI to PNG without windows/devices/network. The resulting review fixed hidden export/error controls, a truncated settings path and a missing Start glyph. Primary actions remain visible in a footer at the minimum viewport. The CPU/GDI native idle/minimized/restore result is documented separately.
- Integer MP4 sample scheduling avoids an extra zero-duration frame at floating-point trim boundaries. Finalized MP4 files are synced after the native sink releases its stream.

## Compatibility and known limits

- Package: `ScreenRecorder-v0.2.0.exe`, Windows x64, static CRT, plus checksums/license notices. Windows Media Foundation H.264/AAC is required; Windows N needs Media Feature Pack. UI rendering uses the CPU and Windows GDI; the isolated idle/minimized/restore check passed.
- Existing video-only projects/settings/edit files remain readable. Keep new `.mic.saudio` and `.system.saudio` files with their `.srec`; the 0.1 application cannot export those audio tracks.
- Audio captures default endpoints, with system loopback covering all applications on that output. No endpoint picker, per-app loopback, monitoring, audio preview or automatic device switching. AVI remains silent. Odd MP4 dimensions pad to even pixels; AAC can add priming/padding.
- Synthetic encoding/decoding, code checks, packaging and checked application-generated CPU PNGs are the available evidence. Real microphone/system capture, long-session A/V sync, device removal/privacy denial, full native keyboard/DPI/interaction behavior and update replacement need manual validation. This is an unpublished candidate, not a hardware-validated release.

## Interface and renderer revision

- Replace the dark teal palette and checkbox lists with neutral light surfaces, labeled vector icons, compact sound switches, three editor effect presets and a native multi-size application icon. Individual effects remain accessible under Customize effects. Add explicit native folder/project pickers and improve slider/scrollbar visibility.
- Replace Glow/OpenGL with CPU rasterization and Win32 GDI presentation after a reported desktop freeze. The native window and bounded static review share the same rasterizer; normal operation is reactive and skips UI rendering while minimized.
- Add explicit isolated renderer diagnostics with CPU/frame/raster/present evidence, finite self-minimize/restore and owned-process timeout. No screen readback, recording, audio endpoint or updater is used. The four five-second idle states generated no layout/raster/presents; visible CPU averaged 1.25% of one core and minimized CPU accounted 0%. See the separate measured native rendering record for tested scope.

- Match the native Windows title bar to the light interface, with per-window palette colors where supported. Verify the light caption mode before/after restore and recheck the four idle states without screen recording or computer use.
