# Windows support

Windows and macOS use the same Rust application. Platform-specific termination,
font loading and system-process protection are selected at build time.

## Windows changes

- Current-user SID comparison through sysinfo; unknown ownership stays protected.
- Protect Windows-directory processes, critical Windows processes, this app and its ancestors.
- Windows explicit force termination using sysinfo's taskkill /PID /F implementation, after refreshed PID/start/executable/ownership checks. No process-tree termination.
- Confirmation dialog explains force termination and possible unsaved work loss.
- Malgun Gothic system font, Ctrl selection, case-insensitive .exe roles.
- Local LLM category for Ollama, llama-server and LM Studio.
- Windows Project/Projects/Repos paths and dated Documents/Codex project paths.
- Available RAM display and global memory sorting on first launch.
- GUI release executable without an accompanying console.
- Optional local diagnostics: process-garden.exe --diagnostics=C:\path\diagnostics.txt
  Outputs only counts and memory totals, never command lines or process paths.

## Build

Use a current Rust MSVC toolchain and Visual Studio C++ Build Tools / Windows SDK.

```powershell
$env:CARGO_BUILD_JOBS = '2'
cargo test --locked -- --test-threads=1
cargo build --locked --release
.\scripts\install-windows.ps1
```

The installer copies the executable to `%LOCALAPPDATA%\Programs\Process Garden`,
creates Start Menu and Desktop shortcuts, and does not add startup tasks/services.
No administrator permissions are requested.

To remove: close Process Garden, delete its installation folder and the two shortcuts.
Build dependencies are separate and are not needed to run the installed application.

## Verification and scope

Eight active tests cover role/path classification, sorting and selection protection,
self protection, unknown ownership, Unicode paths, stale identity rejection, single child termination and selective
batch termination. The ignored test is the sleeping child fixture used by the active
tests; it is not an unfinished test.

Process memory is working set/RSS, not a guaranteed reclaimable amount; shared pages
can be counted more than once. This is a monitor with manual termination, not an
automatic memory cleaner. It does not disable antivirus, startup tasks or services.
No telemetry or AI calls. Unknown/inaccessible processes remain protected.
The PID-only termination API retains a small check-to-use race, as in the upstream
implementation; identity checks reduce risk but do not provide atomic termination.
