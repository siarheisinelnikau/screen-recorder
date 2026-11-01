# ScreenRecorder 0.3.0

ScreenRecorder now offers a translated interface and a choice of light or dark appearance while preserving its recording, editing and verified-update workflow.

## Added

- English, Russian, French, German, Spanish and Italian interface languages, selected by their native names in Settings.
- A coordinated dark palette and a direct Light / Dark selector alongside the language control.
- Automatic persistence of appearance and language preferences, applied without restarting. Existing settings keep English and Light defaults and preserve recording options and startup update preferences.

## Improved

- Recording, editing, export, help, validation, update states and native file-picker titles use the selected language. Existing status messages reformat when the language changes.
- Buttons expand for translated labels and sound-switch descriptions wrap instead of painting over their switches.
- Custom controls, standard widgets and supported Windows title-bar attributes follow the selected palette. Theme changes update native chrome only when the effective appearance changes.
- Message parameters preserve paths, filenames and other user values, including literal brace characters. Technical operating-system, media and network diagnostic details remain unchanged.

## Verification and limits

Catalog completeness/placeholder checks, language switching, settings migration/persistence and existing fixture tests are covered by automated checks. See [the 0.3 verification record](LOCALIZATION_v0.3.0.md) for commands and results.

No GUI, screen/audio capture, microphone, updater installation or device operation was launched for this revision. Real v0.3 screenshots, palette review at supported DPI and visible/idle/minimized/restore measurements remain pending under the repository's code-only restrictions. Existing README screenshots and native measurements are explicitly retained as v0.2 evidence. Previous recording/media limitations still apply.

Windows x64 portable asset: `ScreenRecorder-v0.3.0.exe`, with `SHA256SUMS.txt` and the existing license/notices. Settings and recording projects remain outside the executable and are preserved during updates.
