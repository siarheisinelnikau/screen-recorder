# Native CPU/GDI rendering evidence

Measured on 2026-11-01 after applying `rust-performance` and inspecting the actual host, rasterizer and presenter. The **isolated visible/idle/minimized/restore checks passed**. This is a bounded native rendering result, not certification of all interactions or device paths.

## Latest native caption check

The user's crop exposed a dark native title bar over the fixed light client area. Source `28a4ebe` explicitly requests a light winit window theme before showing the window, then requests palette caption/text colors through DWM on this owned HWND. Unsupported color attributes fall back to the standard light native caption. There is no recurring style update or system-theme change.

The new package SHA-256 is `8bfd6086b22485bedf1b3f1005188ec4babd55c8c0396a65029f8ad87fe6ca8f`. Native diagnostic PID **9868** exited normally after **31.347 seconds**. The native window reported **Light**, and DWM `USE_IMMERSIVE_DARK_MODE` read **0** both at creation and after restore. Caption/text color reads were unavailable (`null`); this verifies light mode, not an exact pixel-color match. No screen readback or computer use was performed.

Four contiguous five-second clean phases again recorded zero UI/layout/raster/present work: visible idle **5.0011 s**, return to idle **5.0018 s**, minimized **5.0016 s**, restored idle **5.0020 s**. Each accounted zero CPU increments under Windows' coarse process-time accounting. Three input-contaminated intervals were retained separately and excluded from idle claims. Restore produced three fresh frames. Presentation errors were zero; the owned process module probe found no probed GPU DLLs. GPU counters remained unavailable. Full evidence: [caption result](performance/caption-result.json), [process identity](performance/caption-process.json).

Formatting, Clippy, 23 synthetic unit tests and the synthetic native H.264/AAC round trip passed on this revision. PE inspection still rejects GPU renderer and dynamic CRT imports. Existing client-area PNGs below document the unchanged UI; they cannot show or verify a native title bar.

The following detailed timings remain the original CPU/GDI baseline, retained with its original executable identity.

## Running path and isolation

- Source `a6c1ec1`; optimized static-CRT x64 `dist/ScreenRecorder-v0.2.0.exe`.
- SHA-256 `d0e17dd6cefb4b13e4df0019a0cbdb203b9a9fa7690721954aaf53199d301809`; PID **44916**; exit **0**, wall **24.636 seconds**, sampled peak working set **52,273,152 bytes**.
- Actual code path: egui 0.37 layout → numeric adapter → egui_software_backend 0.0.2 CPU rasterization → RGB bitmap packing → softbuffer 0.4.8 Win32 **CreateDIBSection / BitBlt**.
- Glow, glutin, eframe and GPU renderer dependencies are removed. PE import inspection rejects OpenGL/DirectX/Vulkan imports. A read-only probe of the owned running PID found none of the probed graphics/driver DLLs. These clues supplement the inspected and timed actual GDI calls.
- Workload: populated real editor, synthetic 640 × 360 preview, 1120 × 800 client area, native scale **1.0**. Fixtures never construct screen/audio capture, normal preferences or updater logic. Capture/export/update dispatch is disabled even if an action were activated accidentally.
- Only the disposable diagnostic window's input is disabled with `EnableWindow`; the opaque GDI presentation style is retained. This provides a clean unchanged workload without recording or synthesizing input. The normal application remains interactive. Window show/minimize/restore are self-issued native calls, with actual minimized state checked every sample. Occlusion information is unavailable on this winit Windows path; no desktop readback was used to assess it.

## Clean state samples

CPU is total process time, including the finite diagnostic controller, expressed both as percent of **one logical core** and normalized across **20 logical processors**. Short Windows CPU accounting is quantized; zero means no accounted CPU increment in that sample, not a proof of literally zero instructions.

| State | Duration | CPU, one core | Machine-normalized | UI / layout / raster / presents |
| --- | ---: | ---: | ---: | --- |
| Visible idle | 5.0014 s | 1.2496% | 0.06248% | 0 / 0 / 0 / 0 |
| Return to idle after explicit repaint | 5.0020 s | 1.2495% | 0.06247% | 0 / 0 / 0 / 0 |
| Minimized idle, native state true | 5.0010 s | 0.0000% | 0.00000% | 0 / 0 / 0 / 0 |
| Restored idle, native state false | 5.0012 s | 0.3124% | 0.01562% | 0 / 0 / 0 / 0 |

There were no input events or interrupted intervals in these samples. Explicit invalidation produced **2** real CPU/GDI frames. Minimize produced **0** frames and one logic-only dispatch. Restore produced **3** fresh frames, then settled. The saved raw record includes every one-second interval and transition.

| Timed stage, including startup/transitions | Count | Median | p95 | Maximum |
| --- | ---: | ---: | ---: | ---: |
| Complete native frame | 34 | 5.290 ms | 9.892 ms | 21.094 ms |
| egui logic/layout | 34 | 0.178 ms | 0.334 ms | 6.074 ms |
| CPU tessellation/raster/bitmap packing | 34 | 3.172 ms | 8.856 ms | 13.638 ms |
| Actual GDI presentation, including buffer copy | 34 | 0.844 ms | 3.750 ms | 4.508 ms |

All 34 samples were retained. Presentation errors: **0**. Isolation failures: **0**. The layout timer is not substituted for complete frame time. Normal operation leaves timing collection disabled and performs no per-frame logging.

[Raw native result](performance/native-result.json) and [process/module/GPU evidence](performance/native-process.json) identify the exact executable. GPU utilization counters were unavailable: `Access to a CIM resource was not available to the client.` No elevation or system change was attempted; GPU utilization is **unknown**, not reported as zero. Windows desktop composition remains an OS responsibility.

## Cause, repair and remaining scope

The earlier Glow path used an NVIDIA RTX 4080 OpenGL context. Some short idle samples had no frames, while other intervals contained substantial real input and rendering. The user reported system-wide freezing during that diagnostic. Those observations did not establish a specific driver/vsync cause, and low CPU was not accepted as sufficient evidence of a fix. GPU launching stopped; the renderer was replaced with genuine CPU rasterization/GDI without changing driver, power or global graphics settings.

A first CPU probe with ordinary input enabled obtained two clean visible samples but ran out of its finite budget after many pointer-motion intervals. Those intervals were excluded from idle claims. The final diagnostic isolates input at the owned window and completes all four states. No FPS cap, priority change or render-thread sleep was added to suppress work. Deadlines coalesce to the next useful wakeup; settled operation returns to `ControlFlow::Wait`. Minimized operation keeps command/worker logic available and does not consume texture deltas through a UI pass.

The runner aborts sustained unexplained idle CPU above 5% of one core or continuous idle rendering, has a 32-second internal sampling budget and a 35-second external owned-PID timeout. All native test instances exited; the final run needed no forced termination.

```powershell
.\scripts\diagnose-render.ps1
```

Ordinary pointer/resize/scroll latency, subjective absence of desktop stutter, screen-reader behavior, mixed-monitor/200% DPI and native file-dialog interaction are not certified by the isolated idle run. Static UI and packaged icon review are documented separately in [RENDER_REVIEW.md](RENDER_REVIEW.md). Real screen/microphone/system capture, playback and updater installation were not performed. Computer use was not used.
