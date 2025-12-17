# Verification

Development date: 2026-10-31 (Europe/Minsk). This session is code-only. No application GUI, desktop capture, microphone/system audio, screenshot, updater installation, external AI service, driver or system-setting operation is authorized or performed.

## Automated scope

Use `scripts/verify.ps1 -Offline` with the existing dependency cache. Tests create isolated files exclusively under this repository's ignored `.test-output` directory. The application main function and live `capture::Source` are never invoked. An early test attempt using the OS temporary directory hit access-denied errors on rename; fixtures now stay inside the writable repository and the checks pass there. Cargo may report that it copied incremental artifacts because hard links are unavailable in this environment; this is a build-cache limitation.

Covered behaviors:

- JPEG-backed project round trip, recovery after a truncated trailing append, dimension/timestamp rejection and source-file non-overwrite.
- AVI headers, chunk/index offsets, independently decoded JPEG frame, and propagation of an injected full-disk encoder error.
- Trimmed export, cancelled export cleanup, pixel effects, source preservation and export destination non-overwrite.
- Edit persistence/replacement and invalid trim rejection.
- Source failure preserves complete frames; pre-stopped/paused control never samples the synthetic source.
- Startup update opt-out persists across settings reloads/replacements.
- Numeric semantic versions, stable-only releases, canonical asset URLs, exact checksum filename matching, payload digest and x64 PE architecture verification on an artificial binary fixture.
- The example writes a 20-frame 320 × 180 artificial recording, a trimmed 16-frame AVI and two seconds of silent mono PCM WAV. An independent Python standard-library parser validates their container structure. Nothing is played or captured.

Final command outcomes and packaged file properties are recorded in the implementation handoff. Safe commands:

```powershell
.\scripts\verify.ps1 -Offline
cargo run --offline --locked --example synthetic --jobs 2
python .\scripts\inspect-fixtures.py
.\scripts\package.ps1 -Offline
```

## Deferred manual validation before publication

- Launch the actual packaged EXE on Windows 10 and 11 x64; inspect Record/Edit/Settings at 100%, 150% and 200% DPI, resize, minimize/restore, keyboard navigation, contrast, clipping and error text.
- Capture **real application and Settings screenshots** and add working image paths to README. No synthetic media or interface mock is a substitute.
- Record deliberately chosen display, window and positive/negative-origin region. Check multi-monitor layouts, moved/occluded/resized/minimized windows, cursor alignment and secure/protected content failure behavior.
- Confirm visible recording/paused/finalizing state, UI controls and Ctrl+Shift+F10 while another application has focus and while paused. No clicks should be collected outside active capture.
- Confirm pause/resume duration, source resize errors, disk-full behavior on a disposable volume, abrupt process interruption, recovered-tail warning and playback in independent AVI players. Real disk-full/power-loss recovery has not been exercised.
- Export long/high-resolution clips up to bounded limits; check timing, zoom crop edges, manual overlap priority, fast clicks, large-file error cleanup, preview/export agreement and cancel/close responsiveness.
- Measure release-build idle CPU with the window visible/minimized; measure capture/encoding frame time and peak memory at intended resolutions. No CPU percentage or real-time throughput guarantee is established by compilation or fixture tests.
- Confirm that startup opt-out emits no update request, manual checks still work, unpublished/404/rate-limit failures stay unobtrusive, and available updates remain discoverable.
- Once real releases exist, test digest mismatch, wrong architecture, interrupted/cancelled download, a read-only install folder, update replacement/backup/rollback/restart and settings/project preservation in an isolated portable installation. Never test replacement on the development executable or real user data.
- Check static CRT linkage, OS DLL imports, OpenGL availability, unsigned-binary prompts, license/notice packaging and full clean-machine behavior. CI configuration is supplied but has not been run on GitHub.

## Known functional limitations

Silent recording only; no MP4/H.264, audio muxing, webcam or isolated/hidden window capture. Standardized tutorial pointer rather than the exact system cursor artwork. Button states sampled at the capture rate can miss short clicks. Automatic zoom extends a nearby segment at its original focus rather than tracking every subsequent click. Single-page editor previews still frames, with no playback transport. Input/output selection uses editable paths rather than native file dialogs. Hardware/capture functionality is implemented but unverified on a live desktop. Update checks/install are implemented but unverified against a published repository. Screenshots are unavailable under the current instruction.
