# ScreenRecorder 0.3.0 localization and appearance verification

## Implementation

The built-in English base and five translation columns cover 159 application messages. Settings exposes native language names and direct Light / Dark choices; changes save once per explicit change and request a finite repaint. Message identity and parameters are retained, so status text follows later language changes. User paths, filenames and native window titles are never translated. Technical OS/codec/network diagnostics remain original, under a localized error heading and expandable details where appropriate.

The palette covers backgrounds, surfaces, text, borders, selection, buttons, inputs, sliders, sound switches and success/warning/error states. Native Windows theme and DWM caption colors update only when the effective palette changes. Unsupported DWM attributes are ignored without changing the system theme. Translated custom-button widths are measured and switch descriptions wrap within reserved space.

The old settings format loads with English and Light while retaining audio preferences, output folder, frame rate and startup update opt-out. Unknown language/theme values fall back to the same defaults. Catalog tests reject blank translations or missing/changed message parameters and exercise a live language change and literal braces in user values.

## Checks

Commands ran with a repository-local `target`, two Cargo jobs and the shared three-build concurrency gate:

- `cargo fmt --all -- --check` — passed.
- `cargo check --offline --all-targets` — passed; generated the version-matched lockfile.
- `cargo test --offline --locked --all-targets` — passed: 21 library tests and 3 binary tests. The existing explicitly opt-in synthetic Media Foundation test remained ignored; no media behavior changed in this revision.
- `cargo clippy --offline --locked --all-targets -- -D warnings` — passed after removing one needless reference.
- A source/catalog audit found all localization lookup keys in the built-in catalog: 159 English messages and 795 non-English translations; no missing callsites.
- `git diff --check` — passed.

Source palette calculations (WCAG relative luminance) give the following contrast ratios. These are numeric role checks, not a rendered-screen review:

| Role | Light | Dark |
| --- | ---: | ---: |
| Body text / background | 14.21:1 | 15.40:1 |
| Secondary text / background | 5.23:1 | 8.94:1 |
| Accent / selected surface | 5.08:1 | 4.71:1 |

## Source performance review and limitations

The inspected renderer remains egui 0.37 meshes/font textures → CPU `egui_software_backend` → retained bitmap → softbuffer Win32 GDI. It creates no OpenGL/DirectX context. The event loop returns to `Wait` after finite repaint deadlines. Hidden/minimized windows execute worker/close logic without layout, tessellation, rasterization or presentation. Theme/catalog changes add no recurring timer or hidden rendering; native chrome changes are guarded by the previous effective theme.

No application launch, device access, screenshots, screen capture, audio capture/playback or updater execution was performed. Repository instructions prohibit those actions in this session. The rendering/load gate is therefore incomplete for this binary: actual idle/hidden frame counts, CPU/GPU usage, DPI clipping, native dialogs, theme persistence through an interactive restart and visual contrast still require a bounded authorized runtime check. Existing images and performance measurements belong to the earlier documented version and are not evidence for v0.3.0.

## Hardcoded string audit

Record, Edit, Settings, action/footer states, native picker titles and generated display/region metadata use the catalog. Remaining literals are product names, file formats/extensions, numeric/SI symbols, internal egui IDs, developer-only CLI/diagnostic output, fixture data or preserved technical OS/media errors. No tray surface exists in this application.
