# ScreenRecorder

Repository: `screen-recorder`
Priority: 13 of 15 (user-requested launch order)
Product: ScreenRecorder
Purpose: Readable screen tutorials with cursor emphasis and automatic zoom
Inspiration: https://screen.studio/

Independent open-source implementation; no affiliation, competitor code or copied assets.

## Requested implementation

Create a Windows screen-recording app with explicit window/region/display selection, start/pause/stop, microphone/system-audio options, visible recording state, output selection and a simple editor for trimming, cursor highlights/click emphasis and automatic or editable zoom. Prefer Windows capture and media encoding APIs and a single packaged EXE where feasible.

Implement real capture/encoder/editor boundaries in source and exercise them with synthetic frames and silent audio fixtures only. Do not record the current desktop, access webcam/microphone, emit audio, install codecs, launch GUI or ask capture permissions in this unattended task. Cursor tracking must be scoped to a user-started recording. Keep bounded queues, cancellation, disk-full recovery and graceful finalization. Document any unsupported encoding or recording mode accurately rather than making a simulated recorder look production-ready.

## Packaging and publication target

Windows standalone asset: `ScreenRecorder-vVERSION.exe` (for the initial release, `ScreenRecorder-v0.1.0.exe`). Append architecture only when distributing multiple executable architectures in the same release.
Planned canonical repository: https://github.com/siarheisinelnikau/screen-recorder (not yet created or published; keep availability/error reporting truthful).

## Working agreement

Follow AGENTS.md. Complete as much real implementation and safe noninteractive verification as possible in this unattended code-only session. Manual UI, device and hardware tests will happen later. Record required external components and their verification status. Keep changes committed locally in meaningful increments.

