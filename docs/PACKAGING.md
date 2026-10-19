# Test packages: running the build on other PCs, and bringing failures back

`scripts/package.py` turns the build into one folder that starts on another Windows 10/11 x64 PC without Python, the
build tools or the repository. Every run on that PC leaves a log folder and a report zip. On this PC the report can be
replayed with the same build, save, settings, random seed and pad input, and its minidump opens with names.

```
python scripts/package.py                          build/dist/ssbu-<date>-<commit>-<build id>/
python scripts/package.py --romfs none --zip       without data.arc, plus a zip of the rest (an update for a PC that has romfs/ already)
```

| option | default | |
|---|---|---|
| `--romfs auto\|link\|copy\|none` | `auto` | data.arc (17 GB): a hard link when the output is on the drive of the dump, else a copy; `none` leaves it out (the other PC sets `romfs =` in `launcher\launcher.ini`). The rest of the RomFS folder (`.nrr\*.nrr`, 13 files of 4 KB) is always packaged, also in `--zip` |
| `--cache none\|containers\|all` | `containers` | `containers` = the shader-container store (made from data.arc + main.elf, valid on any GPU; saves the first start building it). `all` adds the translated shaders of `cache/vk` (2.4 GB; not yet tried on another GPU). The driver's pipeline cache is never packaged |
| `--save <userdata>` | the repository (`<repo>/save`) | the package starts with that folder's `save/` (e.g. a fully unlocked one); `none` = a fresh save |
| `--smoke-save <userdata>` | `build/userdata` | the save `SmokeTest.bat` starts from (a copy; the package's own save is not touched) |
| `--config` | off | ship `build/config/*.ini`; without it the game writes its defaults on the first start |
| `--zip`, `--hash-romfs`, `--force`, `--allow-stale` | | |

The script refuses a stale build: a `runtime/` source that differs from its copy in `build/work`, a copy newer than the exe, or
an exe without its `.pdb` / `.map` (`tools/lifter/build.py link` writes both since 2026-10-15).

## Optional resource-package support

Resource mods are off by default. Build with `python scripts/setup.py --only link --with-mods` to enable them, or `--without-mods` to disable them. The same options work with the initial full setup command. A normal build does not require Rust/Cargo for this extension.

The packager reads `<exe-base>.build.json` and verifies the executable's SHA-256 before deciding what to include. A missing/invalid host record requires relinking; `--allow-stale` does not bypass this feature check. No extra packaging flag is needed:

- Disabled: omit `ssbu_mod_backend.dll`, its build record, notices, PDB and `mods/` even if stale copies remain in the build folder.
- Enabled: include the backend DLL, its build record and license notices; include its PDB when available. The backend must match its sources/build record, and the required notices must exist.
- `manifest.json` records `features.resource_mods` and `mod_resources` (enabled state and relative install/profile/cache paths).

**Install resource mods into `<package>/mods/<mod-name>/`, beside `game/`.** For example:

```text
<package>/
  SSBU Console.exe
  mods/
    README.txt
    MySkin/
      fighter/...
      ui/...
      config.json        # retain it if provided by the author
  game/
    ssbu_native.exe
    ssbu_native.build.json
    ssbu_mod_backend.dll
  config/mods.json       # created when saving a mod profile
  cache/mods/            # created by the backend
```

`fighter`, `ui`, `sound`, `stage`, `effect`, `stream;` and other resource directories belong directly under the individual mod folder. If an archive contains `ultimate/mods/MySkin/`, copy its `MySkin` folder here. Do not nest the entire `ultimate/mods` hierarchy or install into `game/mods` or `romfs`.

The packager creates only the install instructions, not a copy of locally installed third-party mods, development profiles or mod caches. Restart the game after changing mods. The console works with both build variants. Its Mods page checks the host record and executable hash, identifies package contents, and manages aliases, enable flags, virtual costume mappings, ZIP/folder imports, the per-package Explorer shortcut, and deletion into `mods/.trash`. Unsupported builds show a disabled state. Management writes are blocked while the game is running. The full matching CJK fallback font covers dynamic mod names beyond the static UI subset. For a temporary opt-out in an enabled build, set `SSBU_RESOURCE_MODS=0` in `[env]` of `launcher/launcher.ini`. Downloaded Switch `.nro` plugins are not supported by this resource backend. PC Hook DLLs use the independent `SSBU_MODS` interface.

## What is in a package

```
Start.bat  SmokeTest.bat  Replay.bat  Dump.bat  README.txt (for the tester, Chinese)  manifest.json
launcher\launch.ps1 + launcher.ini     Windows PowerShell 5.1; settings of the runs
game\                                   ssbu_native.exe (+ .pdb, .map), ssbu_game_eh_o1.dll, nro_bin_eh_o2\*.dll, SDL3, glslang, opus,
                                        msvcp140 / vcruntime140 / vcruntime140_1 (the MSVC runtime glslang and opus need)
binaries\13.0.4\                        main.elf, main.npdm, sdk.elf
sysdata\                                fonts                   romfs\  data.arc, .nrr\          cache\  shader caches
tests\                                  smoke-test sequence and save          config\ (dlc.txt)  userdata\ logs\ reports\ tmp\
```

The layout is the one of `build/` (the exe runs in `game\` and finds `..\binaries`, `..\cache`, `..\config` as it does in
`build\work`). The exe keeps the name `ssbu_native.exe`: under another name the window is off-screen and the sound muted.

**The whole RomFS folder is needed, not only data.arc.** `.nrr\*.nrr` beside data.arc list the script modules
(fighters, stages) `nn::ro` may load; the game registers them at start. Without them it reaches the menus, then every
module is refused (`nn::ro::LoadModule: ... is in no registered NRR: refused (0x1216)`) and the game waits for ever on the
loading screen before the first fight, with almost no CPU use. The first packages of 2026-10-15 had only data.arc; since
then package.py packages the whole folder and stops when the dump has no `.nrr`, the launcher refuses to start without them,
and the exe itself (`boot.c` `check_romfs`) says so in a message box.

`manifest.json` holds the **build id** (a hash over the binaries in `game\` and `binaries\`), the commit (and whether the
tree had uncommitted changes), the size and sha256 of every file, the link stamps and pdb GUID of every PE file, and size
and time of the data.arc the build was made with. The launcher checks the sizes before every start (`-Verify`: sha256
too) and warns about a damaged or half-copied package and about a data.arc of another size.

## A run (launch.ps1)

1. A session folder `logs\<date-time>_<PC>_<mode>\`.
2. `preflight.txt` stops the start (FATAL) without data.arc, without `.nrr\*.nrr` in the RomFS (or with fewer than the
   manifest lists) or without Media Foundation (`mfplat.dll`: Windows N), and warns about: no Vulkan driver, missing
   "VP9 Video Extensions" / "Web Media Extensions" (Microsoft Store: the codecs Media Foundation decodes the game's movies
   with; the package cannot carry them), no `icu.dll` (Windows 10 before 1903), a data.arc of another size, a non-ASCII or
   long path, little disk space, a damaged package.
   `system.txt`: Windows version, CPU, memory, every GPU with driver version and date, displays, the Vulkan loader and its
   implicit layers (overlays of Steam, OBS, Overwolf... are a common cause of trouble), `vulkaninfo --summary` when it is
   installed, `nvidia-smi`, controllers, power plan, language. `preflight.txt`: data.arc, Vulkan, path, disk space, package.
3. The state the run starts from: `config_before\` and `save_before.zip` (up to `save_snapshot_max_mb`).
4. The game is started with the variables of `session.json` (`SSBU_*` variables of the calling environment are removed and
   listed there): `SSBU_REPORT_DIR` (crash and freeze reports and dumps into the session), `SSBU_TRACE`, `SSBU_REC`
   (input recording), `SSBU_FIXED_SEED` (a random number per run, recorded), `SSBU_DETERMINISTIC`, `SSBU_FIGHT_HASH`,
   `SSBU_MINIDUMP`, plus the `[env]` section of `launcher.ini`. All output goes to `game.log` (stdout and stderr in order).
5. After the exit: exit code and its meaning, the result (`ok`; `warning` = a freeze report or a `Dump.bat` dump in the
   session; `error` = exit code not 0, crash, guest trap, untranslated instruction, device loss, Vulkan fatal error, a
   refused script module, a `rom:/` file not found in `boot_trace.txt`; smoke: `pass` / `fail`), the log lines that decided it, `events.txt` (application errors of the game, and every System-log error or warning
   of the run's time: display driver resets (Display 4101), WHEA hardware errors, disk errors), `config_after\`.
6. `reports\<session>.zip`. On an error the tester is asked for one line of what they were doing (`note.txt`) and Explorer
   shows the zip.

`SmokeTest.bat` is a boot check: no input, the game runs `smoke_seconds` (90) from the opening movie on, with a picture of
the screen every 20 s (`frame_*.bmp`: the same frames on every PC, for comparing GPUs). PASS = exit code 0, no crash, trap,
device loss or freeze report, the opening movie started, the whole time ran. It uses a copy of the save (`tmp\`, removed
afterwards). `SmokeTest.bat -Match` plays the scripted fight of `tests/seq_battle_play_fast.txt` instead (PASS = the debug
end after 20 s of fight) - **not usable on 2026-10-15**: the test sequences find the screens by their draw counts and those
of the build no longer match (the opening movie draws 18, `seq_battle_play*.txt` wait for 22-26; uncapped, loading screens
also outlast the sequence). Both the build of that day and the one before it failed `seq_battle_play.txt` and
`seq_battle_play_fast.txt` the same way; the sequences have to be made again for the build.

`Dump.bat` writes a minidump of the running game into its session (for a hang the freeze watchdog does not report: frames
still presented, game logic stuck).

Test variables that try the reporting of a PC without a real failure (`[env]` of `launcher.ini`):
`SSBU_CRASH_TEST=<s>` writes to address 0 after s seconds (crash report, minidump, exit code 4, result ERROR);
`SSBU_VK_WD_TEST=<frame>,<s>` holds the render thread (freeze report and minidump, result WARNING).

## The runtime's part

- `SSBU_REPORT_DIR=<folder>`: `<exe>_crash.txt`, `<exe>_freeze.txt` and the dumps go there instead of next to the exe.
- A crash writes `<exe>_crash_<time>.dmp` after the text report; the freeze watchdog writes `<exe>_freeze_<time>.dmp` with its
  first two reports. `SSBU_MINIDUMP=0` none, `=full` the whole memory (several GB). Dumps hold all threads, registers, stacks,
  the memory they point to, and the module list. The dump is written by a second process (`ssbu_native.exe --write-dump
  <pid> ...`, hostrt.c `diag_dump_helper`) while the faulting thread waits: written from inside, `MiniDumpWriteDump`
  suspends the other threads first and hung when one of them held a lock it needed (0-byte file, game never ended).
- A second thread that faults while the first one reports waits; a fault inside the report ends the process (exit 4).
- The crash report lists the loaded modules (address range and link stamp).
- The first lines of every log name the exe and the DLLs beside it with their link stamps, the PC and the time.

## Bringing a failure back

Keep the package folder (or at least `build/dist/symstore`) of every build that went to another PC.

**Replay.** Drag the report zip onto `Replay.bat` of the package of the same build (the launcher compares the build id and
refuses another build unless `-Force`). It starts from the report's `save_before.zip` and `config_before\` (`-LocalConfig`:
this PC's settings), with the run's variables (seed, deterministic mode, test settings) and `SSBU_PLAY=input.rec`, and at
the end compares `fight_hash.txt` line by line with the original: `IDENTICAL`, or the first fight frame that differs.

Limits, so the result is read right:
- Only player 1's pad is recorded (`nnhid.c`). Other players' pads and the keyboard of other players are not.
- Input is replayed by screen anchors (draw-count classes) and polls; the fight is reproducible only as far as the game is
  deterministic: `SSBU_DETERMINISTIC=1` (the launcher's default) fixes the script random generators and the extra logic frames,
  but a fight had not been shown frame-identical across runs before (tests/DEBUG_MODE.md). The fight hash says where a
  replay leaves the original.
- Timing-dependent failures (driver, threads, loading) may need several replays or not come back at all: then the minidump
  and the logs are what there is.

**Minidump.** `python scripts/dumpinfo.py <file.dmp>` prints the exception, the modules with their link stamps, whether the
symbol store has each of them (and the exe's `.pdb`), and the build and commit the dump comes from. In WinDbg:

```
.sympath srv*D:\pcsmash\build\dist\symstore
.exepath srv*D:\pcsmash\build\dist\symstore
.reload
!analyze -v
~*k
```

The store has every packaged exe / DLL by link stamp and image size and the exe's `.pdb` by GUID (the layout of a Microsoft
symbol server), so the dump finds its own build. Host functions of the exe get names from the `.pdb`; the lifted game
DLL and the script modules have no debug information: their frames are `ssbu_game_eh_o1+<offset>`, and the guest functions
(`f_<address>`) are in `ssbu_native_crash.txt` / `_freeze.txt` of the same session. Visual Studio: open the `.dmp`, add the
store folder under Tools > Options > Debugging > Symbols.

## Notes

- Known gap: `SSBU_FIGHT_HASH` names each fighter's line by its entry id, looked up as `FIGHTER_INSTANCE_WORK_ID_INT_ENTRY_ID`
  in `game\fighters\sdk_generated\lua_const.h` (fighter_host.c `fd_value`). That header is produced by the fighter SDK tools
  and is not in this build (nor in `build/work`): every line says `e -1`, the log has `[debug] cannot open ...lua_const.h`.
  Lines are still per fighter and in order; the comparison of two runs works.

- First start on a new PC: the game builds about 3-4 GB of shader caches in `cache\` and stutters for about 15 minutes of play.
- The package path should be ASCII (the runtime opens files with ANSI paths) and short.
- Closing the launcher's console window while the game runs leaves the game running but the report is not made.

## Required player name

Before starting any game mode, fill in **游戏内名字** on Console's **开始** page.
It is saved as `[run] game_name` in `launcher/launcher.ini` and passed to the game
as `SSBU_NICKNAME` for ordinary play, online play, and smoke runs. A blank name
blocks startup; no Windows account name or generated default is used. Names
allow at most 32 UTF-8 bytes and cannot contain control characters.

Existing explicit `[net] game_name` values are read when the new key is absent.
Clearing the new field writes an empty canonical value and prevents the old name
from returning. Network room nicknames remain separate. The PowerShell launcher
uses the same rule; replay may use the explicit name recorded in its session.
Direct executables also reject missing/invalid `SSBU_NICKNAME`. Developer Python
launchers accept `--nickname` or the name saved in the selected build directory.

Console starts normal gameplay immediately. Online connection is optional after
startup: open **联机**, connect to a server, then select local wireless in game.
There is no separate online launch option. The online page returns to Start when
the game is not running; it never launches or connects automatically.


## Independent optional extensions (2026-10-18)

The normal setup link step now accepts `--with-latency-slider` / `--without-latency-slider`, independently of
`--with-mods` / `--without-mods`. Both default off; see [BUILDING.md](../BUILDING.md#optional-extensions-on-yuzu)
for all four combinations. The host record uses schema 2; schema 1 remains readable with delay support treated
as disabled.

Packaging uses the options recorded for the exact host binary. Delay-enabled packages include
`game/plugins/latency_slider.dll`, `game/net_delay/delay_panel.exe`, the web assets, shared Console fonts and
licenses, the WebView2 license, and the extension build record. The record is bound by hash to the host build
record. Packaging rejects changed or missing extension artifacts, mismatched sources, or missing licenses;
`--allow-stale` does not bypass these integrity checks. Disabled extensions are omitted even when old build
outputs remain. Delay preferences and browser cache are runtime data, not prefilled package assets.
