# onezan: hand checks still to do

The hand checks owed by work that is done in code but not yet tried by hand, newest first. Tick
one off by moving it to "Done" with the date, the system and the commit. Things found while
checking go into STATUS.md ("Known problems").

This is the to-do list. RELEASE.md's hand checklist is the full pass over a release build; it
still has to be done for 0.3.0, even for checks done here.

## Owed

### Plugins that crash onezan (branch `polish-0.3`, 2026-11-27)

Done in code, tested automatically, and from the command line on Linux and under Wine. Not yet
seen in the window, nor on real Windows.

Setup: `build/test-plugins/make.sh` builds two bundles that crash on purpose (`make.sh windows`
for Windows, with MinGW). Use a data folder of your own, so the real one stays untouched:

```sh
cargo build
build/test-plugins/make.sh
VST3_PATH=target/test-plugins XDG_DATA_HOME=/tmp/onezan-qa XDG_CONFIG_HOME=/tmp/onezan-qa target/debug/onezan
```

On Windows, copy `target/test-plugins-win` over, set `VST3_PATH` to it and move
`%APPDATA%\onezan` aside first.

| Check | Linux | Windows |
|---|---|---|
| First start: onezan crashes (CrashScan, while scanning). Second start: crashes again (CrashLoad is the first instrument). Third start: plays normally | | ✓ VM 00a4bc5 (command line) |
| The bar's Instrument hover: CrashLoad and CrashScan are listed as not used, saying why | | |
| Instruments window: "Crash On Load" is greyed out with "(crashed onezan)", the hover says why; "CrashScan.vst3" is under "Skipped" | | ✓ VM 00a4bc5 (hover not seen) |
| Clicking "Crash On Load": onezan crashes; the next start is normal again | | |
| "Try again" on CrashScan: onezan crashes; the next start is normal again | | |
| A plugin used last time that crashed: the next start plays another one, and the hover says why | | |
| With your real plugins (no test bundles): the second start is noticeably quicker when you have many plugins; Rescan finds a newly installed one | | |
| No crash dialog stays up on Windows after a crash (Windows Error Reporting may show one: note what it says) | — | |

### Found on the Windows VM (2026-11-27)

The Windows 10 VM (`claude@192.168.122.44`, see "The Windows VM" below) has no OpenGL driver;
onezan ran there with Mesa's software OpenGL. Each finding is in STATUS.md ("Known problems").
Check again once fixed:

| Check | Linux | Windows |
|---|---|---|
| Without OpenGL 2.0 (no graphics driver, a VM): onezan says why it cannot start, instead of exiting silently | | |
| On a 1024×768 screen the window fits: the bar's right end and the keyboard's finger numbers are visible (now they run off the screen and under the task bar) | | |
| The startup report's hints name what the window has ("click Audio", "click Timing" are under Options now) | | |
| `onezan list` with nothing found: the folders are shown as paths (on Windows now `"C:\\Program Files\\..."`) | | |

### From before the branch (listed in STATUS.md until 2026-11-27)

"earlier": STATUS.md listed these for real Windows only.

| Check | Linux | Windows |
|---|---|---|
| The clef column in the scales window | earlier | ✓ VM 00a4bc5 |
| The exe's icon in Explorer and the task bar | — | task bar and title bar ✓ VM 00a4bc5; Explorer open |
| Falling notes (playing a song along, waiting) | earlier | |
| A MIDI keyboard unplugged and plugged back in while running | earlier | |
| Falling notes: the file dialog, dropping a file on the window, Space in the notes view | | |
| Falling notes: the mode button at narrow window widths (the bar's left part is cut off first) | | |
| Exact vs. immediate MIDI timing, by ear (needs short audio blocks: JACK or a low-latency PipeWire setup) | | |

## The Windows VM

A Windows 10 22H2 VM (German) on the host, reachable as `claude@192.168.122.44` with the SSH key
of this machine; VirtIO guest tools installed. It has no OpenGL driver: copy Mesa's
`opengl32.dll` and `libgallium_wgl.dll` (mesa-dist-win, x64) next to `onezan.exe` and set
`GALLIUM_DRIVER=llvmpipe`. Command-line checks run over SSH. The window only shows in the
logged-in session: the scheduled task `onezan-qa` runs `C:\Users\claude\qa\run.ps1` there,
which starts onezan, clicks and takes screenshots as `qa\steps.txt` says (`build/windows-vm/`
has the scripts). Audio there is the VM's emulated device: not for latency or timing by ear.

## Done

(nothing yet)
