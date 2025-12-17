# ScreenRecorder 0.1.0 — release draft

First release candidate for a portable Windows tutorial recorder. No preceding published release exists; this draft describes initial capabilities. No release has been published during implementation.

## Added

- Select an explicit display, visible window or pixel region and record to a recoverable project with pause/resume and stop controls.
- Trim recordings and preview cursor highlights, click rings and smooth automatic zoom. Add manual zoom segments with editable timing, focus and scale.
- Export edited silent Motion JPEG AVI without an external codec tool, with progress, cancellation, and preservation of source media and existing output files.
- Recover complete project frames after an incomplete trailing write; retain usable frames if the capture source fails.
- Save recording defaults, check stable GitHub releases manually or at startup with a persistent opt-out, and explicitly download/install/restart from the app with SHA-256 and architecture checks.

## Package

- Intended standalone Windows x64 asset: `ScreenRecorder-v0.1.0.exe`; checksum asset: `SHA256SUMS.txt`.
- Built with Rust/MSVC and a static CRT. No external FFmpeg, service, driver, AI model or API key required. Working desktop OpenGL is required by the UI.
- Keep the MIT license and third-party notices with the executable. This initial binary is unsigned.

## Known limits and release gate

Audio options are unavailable; recordings and exports are silent. No MP4, webcam, hidden-window capture or external-video import. GDI captures visible screen pixels including occlusion. Recordings support up to 4096 × 2160, 5–30 target fps, 200,000 frames and 16 GiB per project; AVI export is capped below 2 GB.

Synthetic pipeline and build checks are the available evidence. Actual packaged UI, hardware capture, independent-player playback and update replacement still require manual validation. Real application/settings screenshots must be added to README. Do not describe this candidate as hardware-tested or publish it as a validated stable build until [the checklist](VERIFICATION.md) is complete.
