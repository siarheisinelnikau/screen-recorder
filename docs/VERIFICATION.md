# Verification

Development date: 2026-10-31 (Europe/Minsk). The active instructions prohibit GUI launches, computer use, real desktop/audio capture, audio playback, updater installation, drivers/services and system changes. None are performed. Native codec testing is confined to CPU encoding/decoding of artificial pixels and silence, with no capture endpoint or renderer.

## Safe commands

Run inside this repository; Cargo is capped at two jobs with a local target directory:

```powershell
.\scripts\verify.ps1 -Offline -NativeMedia
cargo run --offline --locked --example synthetic --jobs 2
python .\scripts\inspect-fixtures.py
python .\scripts\inspect-mp4.py
.\scripts\package.ps1 -Offline
python .\scripts\inspect-package.py
```

`-NativeMedia` explicitly runs the otherwise ignored Media Foundation integration test. Without it, normal synthetic unit tests, formatting and Clippy still run. No test calls `capture::Source::new` or `audio::Session::prepare`; neither video nor audio devices are enumerated or activated. The native test opens only its own artificial media file, disables hardware transforms/DXVA and decodes to memory without playback.

All fixtures live under this repository's ignored `.test-output`. OS temporary-directory renames failed under the sandbox in the initial session, so fixture files stay in the writable repository. Cargo may warn that incremental cache hard links are unavailable and fall back to copying; this is an environment limitation, not a code lint.

## Covered behaviors

- Video project JPEG round trip, incomplete-tail recovery, source size/timestamp rejection, preservation after a synthetic source failure and no source overwrite.
- AVI headers/chunk indexes/JPEG decode, trim/effects, cancellation, existing-output preservation and an injected full encoder sink.
- Shared clock exclusion of pauses and clipping across start/pause/resume/stop; 44.1-to-48 kHz silent resampling with packet phase continuity.
- Audio journal round trip, recovery of complete packets, sparse silence, bounded reads, malformed lengths/timestamps/layout/gain rejection and overflowing-input rejection.
- Integration of audio sidecar discovery with project recovery warnings, and compatibility with pre-audio settings/edit files. Legacy settings leave microphone/system sound disabled.
- NV12 top-down layout, limited-range luma/chroma and odd-edge padding.
- Native MP4 H.264/AAC encode/decode: 127 × 71 artificial frames pad to 128 × 72; a 0.1–0.4 second trim decodes to exactly 3 H.264 frames (0.300 seconds) and 15 silent AAC buffers (end 0.3199999 seconds). AAC priming/padding is bounded separately. Audio is checked for near-zero PCM after decoding.
- Native MP4 cancellation cleanup, existing destination/partial preservation and video-only export when recorded sound is excluded.
- Saved startup-update opt-out, numeric stable-release selection, canonical asset URL checks, checksum filename/hash and artificial x64 PE verification. No real updater check/download/install is invoked.

## Actual command evidence

Formatting and Clippy with `-D warnings` pass. All **20 synthetic unit tests plus 1 explicitly enabled native integration test pass**. CI runs the same commands but has not been executed on GitHub.

The example creates a 20-frame 320 × 180 project, two silent PCM journals, a trimmed AVI/MP4 and a 2-second silent WAV. Independent Python standard-library parsers confirmed:

| File | Observed result |
| --- | --- |
| AVI | 16 MJPEG frames, 320 × 180, 1.6 seconds, one video stream, valid RIFF sizes/indexes |
| WAV | Mono PCM16, 16 kHz, 32,000 zero samples, no playback |
| MP4 | `avc1` video, 16 samples, 1.6 seconds; `mp4a` audio, 75 samples, 1.5999791667 seconds; valid bounded ISO BMFF boxes |

The packaged `ScreenRecorder-v0.2.0.exe` is built with static CRT. `inspect-package.py` reads its x64 PE/GUI headers and imports without executing it, checks the SHA-256 asset, rejects dynamic MSVC/UCRT imports and checks generated license/font notices. The exact current hash accompanies the executable in `dist/SHA256SUMS.txt`; ignored `dist` artifacts are local handoff files, not a published release. FFmpeg/ffprobe is not installed; the native source reader provides the decoded-media check.

Final local asset: 10,017,792 bytes; SHA-256 `7ee0747f2df0253d4a6fd4bf45c43f83b2e2e7c9a966515253a693591d770ff3`. Its imports include `mfplat.dll` and `mfreadwrite.dll` plus the existing OpenGL/OS libraries. This is static package evidence, not an application launch or hardware compatibility result.

The native tests exposed and verified fixes for two concrete issues: a zero-duration extra frame caused by floating-point trim rounding, and an invalid byte-stream flush after sink shutdown. MP4 frame scheduling now uses integer clock ticks, and filesystem synchronization follows native sink release.

## Rendering/design gate — incomplete

`product-design` and the current `rust-performance` gate have been read and applied to source decisions. The existing renderer is eframe Glow/OpenGL; it is not CPU rasterization. Pinned source selects `Wait`/`WaitUntil` and runs only `App::logic` while hidden. The application has no unconditional repaint loop; progress changes are bounded and previews coalesce. These source observations do not prove runtime behavior.

GUI/self-capture is prohibited, so there are no genuine application/settings PNGs, live layout/contrast/accessibility checks, CPU/GPU measurements, frame/present counters, hidden/restore observations or a runtime backend verification. The gate is **not passed**. The user's preference for verified CPU rendering remains relevant; this feature extension does not replace the existing presenter with another unverified path. Before any future visual launch, follow the bounded diagnostic gate and evaluate the renderer/idle behavior. See [design decisions](DESIGN.md).

## Deferred manual/device checks

- Launch the actual packaged EXE on Windows 10/11 x64 after the rendering gate; test settled idle, interaction, return to idle, minimized/hidden and restore. Verify backend, frame production, memory, input latency and presentation behavior.
- Capture real Record, Edit and Settings images through an authorized finite app-generated capture mode where practical. Review 100–200% DPI, long source/path names, focus/keyboard/accessibility, errors, empty states and expanded/collapsed controls. Add accurate README screenshots.
- Record chosen display/window/region, including negative origins and different DPI. Verify cursor alignment, occlusion/resize/minimize/close failure behavior and Ctrl+Shift+F10 while focused elsewhere/paused.
- Explicitly enable microphone/system sound separately and together on disposable recordings. Verify privacy-denied/no-endpoint errors, silence, mono/stereo/multichannel formats, device removal/change, shared mode, no monitoring, pause/resume and clean device release on stop/close/error. These WASAPI paths compile but have not touched a real device.
- Validate long-session A/V sync, real speech/music resampling/downmix quality, sample-clock drift, startup delay and AAC padding in independent players. System loopback covers all apps on the default output, and protected audio may be excluded by Windows. No automatic endpoint switching is implemented.
- Exercise real disk-full/device-overrun/power-loss behavior using disposable volumes/data. Synthetic tests do not establish hardware recovery guarantees. Keep `.srec`, `.mic.saudio`, `.system.saudio` and `.edit.json` together; interrupted exports may leave partial files.
- Measure high-resolution/long export throughput and cancellation responsiveness, including time spent inside native codec calls. Test missing Media Foundation/H.264/AAC (including Windows N), clean-machine dependencies and unsigned-binary prompts. No codecs or feature packs are installed automatically.
- Validate startup opt-out networking and isolated update download/replacement/rollback/restart once real releases exist. No publication or remote Git operation is performed here.

## Remaining functional limits

Default endpoints only; no device picker, per-app loopback, monitoring, audio preview, webcam or hidden-window capture. AVI is silent. System cursor artwork is replaced with a tutorial arrow; short clicks between video samples can be missed. Automatic nearby clicks extend the original zoom focus. Linear resampling is used rather than a high-quality offline filter. Native calls cannot be forcibly interrupted mid-sample/finalization. Paths are editable fields rather than native dialogs. The implemented device paths, UI/design and rendering performance remain explicitly unverified on actual hardware.
