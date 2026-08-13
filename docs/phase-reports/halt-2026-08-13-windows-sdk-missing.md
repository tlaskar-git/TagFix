# HALT report: Windows SDK missing

Date: 2026-08-13 (UTC)
Round: 01 (TagFix v1)
Branch: feat/v1-foundation
Trigger: toolchain missing (hard halt trigger, brief section "Hard halt triggers")

## What happened

Phase 0 checked the five required components and passed. The MSVC check
verified the Visual C++ compiler toolset (VC.Tools.x86.x64) inside Visual
Studio 2022 Community 17.14.38. That check was too shallow: the Windows SDK,
which supplies the umbrella import libraries (kernel32.lib and friends), is a
separate component and it is not installed.

The gap surfaced in Phase 1 on the first `cargo build`:

1. First failure: `link.exe not found`. Repaired by loading the VS developer
   environment (vcvars64.bat) before invoking cargo.
2. Second failure: `LNK1181: cannot open input file 'kernel32.lib'`.

## Evidence

- No `Windows Kits` directory under `C:\Program Files (x86)`,
  `C:\Program Files`, or `D:\`
- A recursive scan of C:\ and D:\ (depth 6) found zero copies of
  `kernel32.lib`
- vswhere package inventory of the VS 2022 install lists no Windows SDK
  package

Without the Windows SDK, no Windows executable can be linked on this machine.
This blocks every subsequent phase, so per the brief the build halts rather
than installing toolchains unattended.

## What the operator needs to do

Install the Windows 11 SDK, either way works:

- Visual Studio Installer, modify VS 2022 Community, check
  "Windows 11 SDK (10.0.26100.0)" (component id
  `Microsoft.VisualStudio.Component.Windows11SDK.26100`), or
- Standalone Windows 11 SDK installer from Microsoft

No other component is missing. After the SDK is present, rerun the round:
the branch `feat/v1-foundation` already carries the Phase 0 report and the
Phase 1 scaffold (Tauri 2 shell, tray, overlay, hotkey wiring, icons), so the
build resumes at the Phase 1 acceptance check.

## Phase status at halt

| Phase | Status |
| --- | --- |
| 0 preflight | pass, with the correction above: MSVC linker present but Windows SDK absent |
| 1 shell | scaffold written, build blocked at link step, acceptance NOT met |
| 2 to 6 | not started |

Test count: 0 (no phase reached the testing stage).
Exe: none produced.
Downgrades taken: none.
