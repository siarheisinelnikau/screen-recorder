# ScreenRecorder

A Windows-first recorder for readable tutorials: capture a display, visible window or a precise region, then trim the clip and add cursor emphasis and smooth zoom. Independent MIT-licensed implementation, with original UI and code. Inspired by the workflow of [Screen Studio](https://screen.studio/); no affiliation or shared assets.

**Version 0.1.0 is an initial implementation, not a hardware-validated public release.** Recording, editing and encoding are real code paths. Automated verification uses artificial frames only. Real desktop capture, UI interaction and updater replacement have deliberately not been run in this unattended development session.

## Capabilities

| Area | Implemented in 0.1.0 |
| --- | --- |
| Sources | Explicit display, window or numeric region selection; physical virtual-desktop coordinates |
| Recording | Windows GDI capture, 5–30 target fps, pause/resume, stop, visible state and Ctrl+Shift+F10 stop shortcut |
| Storage | Append-only `.srec` projects, JPEG frames and scoped cursor/click samples, recovery of complete frames after an incomplete write |
| Editing | Scrubbable preview, in/out trim, cursor arrow and highlight, expanding click rings, automatic 1.8× click zoom, editable 1–3× zoom segments |
| Export | Streaming Motion JPEG AVI, original dimensions, constant frame rate, cancellation and progress |
| Updates | Installed version, manual check, background startup check with saved opt-out, explicit download/install/restart, checksum and PE architecture verification |
| Audio | **Unavailable.** Microphone and system-audio options are disabled and clearly labeled. No audio device is opened. |

No MP4/H.264, webcam, timeline playback, hidden-window recording, HDR color management, protected-content capture or external-video import is provided. `.srec` is the editable recording format; AVI is the shareable output. There are no external codecs, FFmpeg, drivers, browser extensions, services, AI models or API keys.

## Use on Windows

The intended package is **`ScreenRecorder-v0.1.0.exe`**, Windows x64. Build it below; no repository or release has been published by this session. Windows 10/11 and a working OpenGL desktop driver are intended requirements; actual hardware compatibility remains to be verified.

1. Launch the portable executable manually. In **Record**, select Display or Window and press **Refresh sources**, or enter a Region using physical pixels. Choose a source explicitly.
2. Set an absolute output folder, a new `.srec` filename and frame rate. The folder is created only after **Start recording**.
3. Press **Start recording**. Pause and resume from the app. **Stop and save** or **Ctrl+Shift+F10** finalizes the project. Closing the app requests stop and waits for finalization.
4. The saved project opens in **Edit**. Scrub the preview, set trim in/out, enable effects and optionally add zoom segments at the playhead. Focus X/Y are normalized coordinates from 0 to 1. Manual zooms override automatic zoom; the first matching manual segment wins when segments overlap.
5. **Save edit decisions** writes a separate `.edit.json`; the source recording stays unchanged. Choose a new absolute `.avi` destination and **Export edited AVI**. Export does not implicitly save edit decisions.
6. To resume later, enter a `.srec` path under **Continue an existing recording** and press **Open project**. If an edit sidecar is malformed, correct or rename that sidecar before reopening; the source project is retained.

Keep the selected window visible and unoccluded. Window capture follows its visible screen rectangle, including its frame; it is not isolated window capture. Resizing, minimizing, closing, or moving any part outside the virtual desktop stops recording with a recoverable project. Display recording can include the recorder window; place the controls outside the selected region when needed. There is no hidden capture or global key logging. Cursor position and left-button state are sampled only during an explicitly started, unpaused recording; short clicks between samples may be missed. The stop key chord is also checked while paused.

Capture sizes are limited to 64–4096 × 64–2160 pixels. A project is limited to 200,000 frames and 16 GiB. AVI 1.0 exports stop before 2 GB; trim shorter segments for large recordings. MJPEG files can be substantially larger than H.264 files. Slow capture/encoding reduces the sampled frame rate; timestamps preserve elapsed time and export repeats previous frames to maintain duration. Pause time is excluded.

## Screenshots

**Real application and Settings screenshots are pending.** The user prohibited GUI launches, screenshots and desktop control during this run. No mock screenshots, copied product images or broken placeholder links are included. Capture both views after manual validation and add them here before publication.

## Build and safe verification

Use the installed Rust **1.96.1** MSVC toolchain (minimum 1.95) and Microsoft C++ build tools. All Cargo output stays in this repository's `target`; concurrency is capped at two jobs.

```powershell
# Run from E:\work\github\screen-recorder
.\scripts\verify.ps1 -Offline
.\scripts\package.ps1 -Offline
```

Omit `-Offline` on a machine that needs to obtain dependencies. Packaging compiles a release binary with static CRT for `x86_64-pc-windows-msvc`, without starting it, and creates `dist/ScreenRecorder-v0.1.0.exe` and `dist/SHA256SUMS.txt`. Keep the MIT license and third-party notices with a distribution. The binary is not code-signed. No packaging step publishes anything.

The optional fixture generator also never accesses a desktop or device:

```powershell
cargo run --offline --locked --example synthetic --jobs 2
python .\scripts\inspect-fixtures.py
```

It creates a short artificial project, an edited AVI and a silent PCM WAV under a new `.test-output/synthetic-*` directory. The WAV is solely a fixture for future audio work; it is not muxed into the silent AVI. The Python inspector checks RIFF structure, frame indexes, dimensions, duration and silence without playing media. Tests also cover partial-tail recovery, source failure, a simulated full encoder sink, cancellation, non-overwrite behavior, edit persistence, update selection and verification. See [verification](docs/VERIFICATION.md) for actual results and deferred checks.

## Data and updates

Projects and edits are stored in the chosen folder. Settings are stored in `%LOCALAPPDATA%\ScreenRecorder\settings.json`. Recording and edit data are separate from the executable and are preserved during updates. Nothing uploads recorded media. A startup update check sends a normal public HTTPS request to GitHub only if **Check for updates on startup** is enabled; disabling it prevents that request on subsequent launches. Checks do not download application payloads.

The planned canonical repository is [siarheisinelnikau/screen-recorder](https://github.com/siarheisinelnikau/screen-recorder). It has not been created or published in this run. A missing release, API rate limit or network error is reported as unavailable, not as an up-to-date installation. Manual checks remain available when startup checks are off.

**Download and install (restarts app)** downloads only the newer stable x64 portable asset and requires `SHA256SUMS.txt`. Downloads are size-bounded, verified against the digest and checked for an x64 PE header. A helper waits for the old process to exit, backs up the executable, replaces it and relaunches the same installed path. A failed replacement attempts to restore the original. The application folder must be writable. Update staging folders `.screenrecorder-update-*` retain the previous executable and any error log for manual recovery; they are not automatically deleted. Install failures are recorded in staging and `%LOCALAPPDATA%\ScreenRecorder\update-error.txt`, which is displayed on the next launch. Replacement/restart, interrupted downloads and real networking still require manual validation. Digests are fetched over HTTPS from the same official release; this is not an independent code-signing scheme.

Read the [architecture](docs/ARCHITECTURE.md), [manual validation checklist](docs/VERIFICATION.md) and [initial release draft](docs/RELEASE_NOTES_v0.1.0.md) before distributing this first build.
