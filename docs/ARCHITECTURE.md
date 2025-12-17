# Architecture

ScreenRecorder is a Rust desktop application. `eframe` 0.37 supplies the Windows UI, text and OpenGL presentation; `windows-sys` exposes GDI. `image` supplies JPEG encoding/decoding and CPU resampling. There is no external media process or runtime service.

## Boundaries and lifetime

- `capture::FrameSource` is the frame boundary. The Windows `Source` owns a screen DC, memory DC and top-down DIB. It uses `BitBlt` into the reusable DIB and converts BGRA to RGB. GDI resources are restored/released on every exit path. The source is created and destroyed on the capture worker after the Start action; it is never constructed by a test.
- `recorder` drives a single synchronous capture/encode/write worker. There is no unbounded queue of images. Target fps determines the next deadline; slow work does not enqueue stale captures. Monotonic timestamps exclude paused time. Pause waits on a bounded command channel with a 250 ms stop-shortcut check; it does not sample cursor coordinates or acquire frames.
- `project` appends JPEG frames and cursor metadata, validates dimensions/timestamps/lengths and builds a bounded index when opening. It syncs data every 15 frames and at finalization. A failed append attempts truncation to the last complete frame; a failed capture still flushes completed frames. The reader ignores an incomplete trailing header or body without modifying the source. This is recovery of completed writes, not a guarantee against arbitrary filesystem/power-loss corruption.
- `editor` separates source media from edit decisions. Automatic zoom segments come from sampled left-click edges; nearby clicks extend the same focus segment. Smoothstep entry/exit ramps animate crop position/scale. Cursor highlight, standardized pointer and click rings are applied after crop/resampling. Preview and export call the same compositor.
- `avi` writes AVI 1.0 with a single `vids/MJPG` stream and an `idx1` index. Export decodes at most one source frame at a time, repeats it for constant-rate output if necessary, and writes an adjacent `.avi.partial`. Successful finalization atomically publishes a new destination with `MoveFileW` without replacing a preexisting file. Errors/cancellation remove the partial file; an abrupt process kill may leave it for manual cleanup.
- `settings` stores only recording defaults and startup-update preference. Edits/settings use a complete temporary write, sync, then replacement. A stale temporary file causes a useful error instead of being silently overwritten.
- `updater` owns HTTPS, semantic version selection, compatible asset selection, digest/PE checks, staging and the helper mode. It never runs in the test suite. Pure release/checksum selection and artificial PE verification are tested independently.

There is one media worker and at most one update worker. Both return through an eight-message bounded channel. Progress is throttled to roughly four updates per second and stale progress can be dropped. Worker completion explicitly wakes the UI; no idle timer requests continuous repainting. Slider changes coalesce and preview generation IDs reject stale results. Heavy frame processing, indexing, capture, network and downloads run away from the UI; small settings/edit JSON saves occur on their explicit UI actions.

Closing during recording/export requests cancellation and waits for the media completion event before destroying the window. Closing during update download requests cancellation and waits for completion without launching the helper. A normal idle close does not wait for a metadata-only startup request. There is no tray or hidden persistent process. Windows shutdown/process termination can interrupt finalization; reopening `.srec` attempts tail recovery.

The pinned eframe native event-loop source was inspected: its `check_redraw_requests` explicitly selects `Wait` when no repaint deadline remains and uses `WaitUntil` for deadlines. It includes the polling fix discussed by the performance skill. No custom event-loop workaround or claim of measured idle CPU is made. GPU, frame latency, memory and actual capture throughput need manual measurement.

## SREC version 1 layout

All scalar fields are little-endian. File header: 8-byte `SREC0001`, `u32 width`, `u32 height`, `u32 fps` (20 bytes). Repeating frame records have a 25-byte header: `u32 jpeg_length`, `u64 time_us`, `f32 cursor_x`, `f32 cursor_y`, `u8 flags` (bit 0 visible, bit 1 left-button down edge), four reserved zero bytes, followed by JPEG data. Timestamps are strictly increasing. Coordinates are finite and normalized even when invisible.

The reader caps the file at 16 GiB, frames at 200,000, individual JPEGs at 32 MiB, dimensions at 4096 × 2160, time at 24 hours and decoded-frame allocation at 128 MiB. The index stores metadata/offsets, not frame pixels. JPEG dimensions must match the project header. Edit sidecars are at most 1 MiB and contain at most 2,000 manual zoom segments.

## Design tradeoffs

GDI was chosen as a small Windows capture boundary with no capture-picker permission or driver dependency. It captures visible pixels, including occlusion, and can fail on secure/protected content. It is not Windows Graphics Capture or Desktop Duplication. MJPEG AVI was chosen for a self-contained, inspectable encoder; Media Foundation H.264/MP4 and real WASAPI microphone/loopback support are future work. UI options do not pretend those integrations are present.

Native reference: [Microsoft's GDI capture documentation](https://learn.microsoft.com/en-us/windows/win32/gdi/capturing-an-image). Dependency APIs and the event loop were checked against the installed locked crate sources. The product reference was consulted for the task category only; no competitor code, styles or artwork were reused.
