# Application mark

Original ScreenRecorder artwork, covered by the project MIT license. `ScreenRecorder.svg` is the vector reference. The transparent PNG supplies the winit window/taskbar icon; the ICO contains 16, 20, 24, 32, 48, 64, 128 and 256-pixel variants for the executable resource compiled by `build.rs`.

`scripts/inspect-icons.ps1` maps the packaged EXE as resource data and loads/renders those sizes with Windows APIs, without executing its entry point. The icon is a blue recording display with a white recording dot and stand, matching the vector UI brand mark.
