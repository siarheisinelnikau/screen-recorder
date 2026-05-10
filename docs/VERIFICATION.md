# Verification

Latest verification: 2026-11-01 (Europe/Minsk). The user authorized short isolated GUI diagnostics and prohibited computer use, screen recording and audio-device access. CPU/GDI idle/minimized/restore was measured on a disposable synthetic editor window. No real desktop/audio capture, playback, updater installation, driver/service or system-setting change was performed. Native codec tests use artificial pixels and silence only.

## Safe commands

Run inside this repository; Cargo is capped at two jobs with a local target directory:

```powershell
.\scripts\verify.ps1 -Offline -NativeMedia
cargo run --offline --locked --example synthetic --jobs 2
python .\scripts\inspect-fixtures.py
python .\scripts\inspect-mp4.py
.\scripts\package.ps1 -Offline
python .\scripts\inspect-package.py
.\scripts\render-review.ps1
.\scripts\inspect-icons.ps1
# Explicit opt-in GUI test; no recording/audio/network
.\scripts\diagnose-render.ps1
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

Formatting and Clippy with `-D warnings` pass. All **23 synthetic unit tests plus 1 explicitly enabled native integration test pass**. The three UI-host checks cover coalesced deadlines returning to sleep, native bitmap RGB ordering, and Space activation of a focused switch with correct egui 0.37 texture-delta consumption. CI runs the same commands but has not been executed on GitHub.

The example creates a 20-frame 320 × 180 project, two silent PCM journals, a trimmed AVI/MP4 and a 2-second silent WAV. Independent Python standard-library parsers confirmed:

| File | Observed result |
| --- | --- |
| AVI | 16 MJPEG frames, 320 × 180, 1.6 seconds, one video stream, valid RIFF sizes/indexes |
| WAV | Mono PCM16, 16 kHz, 32,000 zero samples, no playback |
| MP4 | `avc1` video, 16 samples, 1.6 seconds; `mp4a` audio, 75 samples, 1.5999791667 seconds; valid bounded ISO BMFF boxes |

The packaged `ScreenRecorder-v0.2.0.exe` is built with static CRT. `inspect-package.py` reads its x64 PE/GUI headers and imports without executing it, checks the SHA-256 asset, rejects dynamic MSVC/UCRT imports and checks generated license/font notices. The exact current hash accompanies the executable in `dist/SHA256SUMS.txt`; ignored `dist` artifacts are local handoff files, not a published release. FFmpeg/ffprobe is not installed; the native source reader provides the decoded-media check.

The current package checksum is in `dist/SHA256SUMS.txt`; the exact EXE used to generate the checked-in UI images is identified in `docs/screenshots/manifest.json`. Imports include `mfplat.dll` and `mfreadwrite.dll` and OS/GDI libraries; no OpenGL/DirectX/Vulkan renderer imports are present. Static import inspection is not normal-presenter or hardware compatibility evidence.

The native tests exposed and verified fixes for two concrete issues: a zero-duration extra frame caused by floating-point trim rounding, and an invalid byte-stream flush after sink shutdown. MP4 frame scheduling now uses integer clock ticks, and filesystem synchronization follows native sink release.

## Static rendering review — performed

The actual packaged EXE was invoked in a new bounded `--render-review` mode. It constructs no window or OS event loop, reads no normal preferences and starts no workers/network/capture. The same egui 0.37 application components, fonts and layout produce real meshes/textures. A numeric adapter feeds those to an installed CPU rasterizer; this is the same CPU rasterizer as the normal GDI window.

Six PNGs were decoded and visually inspected: Record, Edit, Settings, a 150% error case, minimum-size Edit and 150% Settings. The review found and corrected hidden primary export/error controls, an unnecessarily truncated settings path and a missing Start glyph. Actual button rectangles are now asserted to remain within the viewport. Images, scene dimensions, finite frame counts and executable identity are retained under `docs/screenshots`; [the review record](RENDER_REVIEW.md) states the measurements and scope. CPU time during this finite workload is not idle CPU evidence.

## Native CPU/GDI idle/minimized/restore — passed in isolation

`rust-performance` source and bounded-runtime checks were performed on source `a6c1ec1`, exact EXE SHA-256 `d0e17dd6cefb4b13e4df0019a0cbdb203b9a9fa7690721954aaf53199d301809`. Four clean five-second intervals had **zero UI/layout/raster/present activity**. Visible idle averaged **1.25% of one core** (0.0625% across 20 processors); minimized idle accounted **0%**; restored idle averaged **0.3124% of one core**. Restore produced three fresh frames and settled. The process exited automatically after 24.636 seconds, with no presentation/isolation errors. Actual GDI calls were timed; PE and running-module checks support removal of the GPU renderer. GPU utilization counters were unavailable, not assumed zero.

The diagnostic disables input only for its owned opaque test window; normal input latency is outside this baseline. This is no longer a screenshot-only or source-only gate claim. Full intervals, stage timings, backend/source evidence and remaining scope are in [RENDER_PERFORMANCE.md](RENDER_PERFORMANCE.md), with raw JSON checked in. The old Glow path was removed after the user's report of system-wide freezing; no global graphics configuration was changed.

The final six UI PNGs were inspected at 100%/150%. The EXE's native icon resources loaded/rendered at eight sizes without launching code; 16/32/256 images were visually inspected. The main window is configured with the same artwork. OS shell/cache identity, expanded UI states and native picker interaction remain separate checks.

## Deferred manual/device checks

- Extend the measured CPU/GDI baseline to ordinary pointer, scroll, resize, focus/keyboard/accessibility and subjective desktop responsiveness on Windows 10/11. Recheck rendering/load after relevant executable or scheduling changes.
- Compare the checked component PNGs with an authorized normal-window/native-presenter capture. Extend coverage to 200% OS DPI, focus/keyboard/accessibility, empty/expanded states and additional long source names. The README already contains the reviewed CPU component images; actual GDI lifecycle measurements are retained separately. Native folder/project dialogs were compiled but intentionally not opened during automated checks.
- Record chosen display/window/region, including negative origins and different DPI. Verify cursor alignment, occlusion/resize/minimize/close failure behavior and Ctrl+Shift+F10 while focused elsewhere/paused.
- Explicitly enable microphone/system sound separately and together on disposable recordings. Verify privacy-denied/no-endpoint errors, silence, mono/stereo/multichannel formats, device removal/change, shared mode, no monitoring, pause/resume and clean device release on stop/close/error. These WASAPI paths compile but have not touched a real device.
- Validate long-session A/V sync, real speech/music resampling/downmix quality, sample-clock drift, startup delay and AAC padding in independent players. System loopback covers all apps on the default output, and protected audio may be excluded by Windows. No automatic endpoint switching is implemented.
- Exercise real disk-full/device-overrun/power-loss behavior using disposable volumes/data. Synthetic tests do not establish hardware recovery guarantees. Keep `.srec`, `.mic.saudio`, `.system.saudio` and `.edit.json` together; interrupted exports may leave partial files.
- Measure high-resolution/long export throughput and cancellation responsiveness, including time spent inside native codec calls. Test missing Media Foundation/H.264/AAC (including Windows N), clean-machine dependencies and unsigned-binary prompts. No codecs or feature packs are installed automatically.
- Validate startup opt-out networking and isolated update download/replacement/rollback/restart once real releases exist. No publication or remote Git operation is performed here.

## Remaining functional limits

Default endpoints only; no device picker, per-app loopback, monitoring, audio preview, webcam or hidden-window capture. AVI is silent. System cursor artwork is replaced with a tutorial arrow; short clicks between video samples can be missed. Automatic nearby clicks extend the original zoom focus. Linear resampling is used rather than a high-quality offline filter. Native calls cannot be forcibly interrupted mid-sample/finalization. Folder/project pickers are implemented for explicit user actions; export destinations remain editable fields. Device paths and broad interactive/hardware compatibility remain unverified beyond the stated synthetic and CPU/GDI baseline.
