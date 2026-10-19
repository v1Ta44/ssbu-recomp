# mod-adapt checkpoints

Base: yuzu f877d97. Branch: mod-adapt. Game: 13.0.4.

All game inputs are read-only. Runtime hooks and in-memory archive changes are allowed.
Only backend implementation, public interfaces, build/package integration and focused tests are in scope.

## CP0 — inspected baseline

- Clean worktree; created mod-adapt from yuzu.
- Existing loose layer is experimental and not in the normal link list.
- Reference: Raytwo/ARCropolis bf2005b0282a524f242ae8977a70dad23e801318.
- Separate native package/patch backend from the 13.0.4 runtime adapter.
- Checkpoint Git objects are kept under refs/checkpoints/mod-adapt; final branch will contain one commit.

Further checkpoints record validation and outstanding hardware checks.

## CP1 — native backend compiles

- Independent Rust backend with Cargo.lock; no new frontend code.
- Stable discovery/priorities, profiles, diagnostics, public JSON operations and public patch parsers compile.
- Sparse virtual ARC view keeps all original game inputs read-only; metadata and groups are provided on reads.
- cargo check passed; archive semantics and integration still require verification.

## CP2 — virtual ARC native loading

- Native C filesystem adapter and JSON/pipe interface link in an isolated host build.
- Independent archive reader verifies larger replacement, addition, native search lookup, stream offsets and unaffected resources.
- Fixed primary/auxiliary partition relocation, per-entry 16-byte group padding, parallel FileData slots, and allocation alignment. These were confirmed against the native loader's reads; early failed probes are retained under build/mod-adapt.
- Native game initializes, loads Mario's model, an independently replaced shared skeleton, parameters and an added (unreferenced) texture record, then reaches simulation. No game input file changed (archive, game DLL and main ELF size/mtime checked).
- Six focused Rust tests pass. The existing menu script enters a demonstration battle with this save, so a complete deterministic match comparison is still a manual validation item.

## CP3 — final integration and release

- Release DLL built with locked dependencies; source/DLL fingerprints and dependency notices generated.
- Six fast unit checks pass (0.02 s); one independent reader check passes for replacement, shared skeleton isolation, chained shares, explicit original-file membership, new search paths, streams, native group layout, auxiliary tables, positioned concurrent reads, and restart snapshots.
- Native C ABI check passes with Unicode paths, explicit plugin diagnosis, ownership and progress/status responses.
- Final host-only link succeeds using the original game DLL. No frontend source changed.
- Parallel graphics/focus fixes and crash reports remain in the working tree. The shared nnmisc.c file contributes only the language accessor line to this checkpoint and the final commit.
- Original data.arc, main.elf and game DLL metadata remain unchanged. Full visual/audio/package and complete-match checks remain on the documented manual checklist.
- CP1, CP2, CP3 are stored under refs/checkpoints/mod-adapt; final branch history is one resource-backend commit from the recorded yuzu base.


## CP4 — GameBanana package preparation and manual handoff (2026-10-17)

- Fetched origin/fork; origin/main remains 938e66b and fork/yuzu remains f877d97. Both are already ancestors. Rebase onto yuzu in an isolated checkout reports up to date; there are no new committed upstream fixes to replay.
- Preserved current uncommitted focus/graphics fixes under refs/checkpoints/mod-adapt/retest-fixes. Built the host from that snapshot without committing those fixes into the resource change.
- Downloaded and checksum-verified 11 GameBanana packages. Prepared 8 resource scenarios plus one vanilla baseline, independent saves, manual launchers and per-launch evidence folders under ignored build/mod-adapt/gamebanana.
- Real package validation exposed malformed UTF-16 XMSBT in the Knuckles download. Match ARCropolis discovery behavior: warn and skip that patch, keeping unrelated skin assets and other valid message patches. One targeted regression added; all 7 quick Rust tests and the existing independent archive-reader check pass.
- Every resource scenario prepares successfully through the release DLL; the combined case also prepares for us_en as well as zh_cn. Combined stats: 162 replacements, 157 additions, 3 streams, 13 patch inputs, 44 groups.
- User explicitly took over actual game testing. Stopped the owned preliminary process; no visual/audio/full-match pass is claimed. Manual checklist: docs/MOD_GAMEBANANA_TEST_2026-10-17.md. No further game sessions launched.
- Original data.arc, game DLL and main ELF size/mtime remain unchanged. Downloads remain ignored and are not redistributed.


## CP5 — Sonic load-group regression and manual audio (2026-10-17)

- User reported successful skin portraits/loading layout but prolonged loading after character select; both baseline and skin launches were silent. Other manual groups remain untested.
- Existing skin log stays at 60 fps / 31 draws per frame for about 55 seconds until exit, with no Sonic NRO initialization. Native scheduling at 0x71035456d0 uses companion offsets adjacent to the base DirectoryOffset; rebuilding only the base had redirected its regional NRO request to unrelated data.
- Preserve the entire contiguous regional/localized DirectoryOffset span when cloning a load group. Extend the existing private-archive regression with the native adjacency constraint. Before-fix DLL reliably fails; fixed DLL passes, including all 14 skin and 266 combined companion rows plus bulk layout checks.
- Existing audio logs explicitly show host volume 0 percent: the manual EXE name selects the runtime's silent test-copy default. Move the manual launcher into scripts/play_resource_test.py and explicitly set SSBU_VOLUME=100. Update all prepared launchers and the actual DLL beside the manual host; retain a compatibility entry at the former local play.py path.
- User owns actual game testing; no game sessions started. Only test 2 is requested next, including menu sound, entry time, one short match and re-entry. No claim of a complete in-game fix validation yet.
- Before-fix branch ref: refs/checkpoints/mod-adapt/before-skin-load-fix. Old DLL, metadata and original user logs are retained under ignored build/mod-adapt/gamebanana/bug02 and runs directories. Unrelated focus/graphics work remains untouched.


## CP6 — initial packaged-texture upload lifetime (2026-10-17)

- User's second skin run passes loading and initializes lua2cpp_sonic, then crashes before gameplay. New log confirms host volume 100 percent. Preserved the 06:53:22 dump/report and the user's original session.
- Matching old host map resolves exception RVA 0x15090 to mem_hash + 0x50; raw return addresses also name nvn_front_tex_changed and vt_get2. The captured live Tex is a 512x512, format 0x4d, ten-mip packaged texture with uploaded=0 and data_size=0x55e00; its source address is unreadable. The report does not capture when that memory became unavailable.
- Capture packaged texture bytes synchronously before publishing the initialized host record. GL/Vulkan hash and upload paths use this owned snapshot, then release it after upload; deferred destruction also releases unused snapshots. Invalid initialization sources fail explicitly before publication. No game files or frontend changed.
- One focused native regression uses real initialization/hash/deswizzle code without guest startup, GPU or a window: missing ownership reproduces the access violation; positive case survives source decommit and verifies every byte of all ten mips, release after consumption, unused cleanup, and invalid-source rejection.
- Rebuilt all 52 host files to propagate the shared Tex layout change. Manual launchers keep their paths and independent saves. No actual game run performed by the agent; user is asked to retry only test 2, including visible skin, movement, sound and re-entry.
- Clean owned versions of nvn_gl.c and nvn_front.h exclude the parallel memory-retirement fix. Working copies retain those existing changes. Old crash evidence and new host fingerprint are recorded under build/mod-adapt/gamebanana/bug02-crash/diagnosis.json.


## CP7 — unresolved startup heap failure and reliable fatal capture (2026-10-17 14:00)

- User run 20261017-140004-967659 exits 3221226356 = 0xc0000374, confirmed by Windows application error/WER at 14:00:45. CP6 is not validated by actual gameplay and must not be presented as a complete fix.
- Existing runtime minidump remains dated 06:53:23, from the previous access violation; no fresh heap-failure dump survived. Preserve WER report plus matching failing EXE/PDB/MAP under ignored bug03-heap; do not attribute the new failure using the stale dump.
- Root cause remains unresolved. Audit found no demonstrated corrupting write/free; CP6 packaged snapshot and cleanup remain a suspect, not a proven cause. No speculative further renderer/game patch applied; keep the same failed executable for useful diagnostic capture.
- Added resource_test_debug.py: debugger only for the manually launched skin test child, no registry/global debugger settings. Captures heap/fail-fast before teardown and second-chance exceptions, passes handled first-chance faults through, saves x64 context and per-session matching symbols. Other manual groups retain ordinary startup.
- One independent native probe passes: repaired access violation handled by child; synthetic 0xc0000374 fail-fast captured into a new valid dump with matching thread/exception/RIP. This verifies capture, not the game's heap-failure cause. No game or GPU session started by the agent.
- Next evidence requires only one manual diagnostic startup of test 2, not another complete match/checklist. All later test groups stay pending. No game inputs modified and unrelated working changes preserved.


## CP8 — lifetime diagnosis audit (2026-10-17)

- Rechecked original lifecycle logs and the upstream-only build result at 938e66b: compute copyback after finalization and page decommit is established; it predates resource adaptation. Existing retirement drains compute buffers, not every deferred resource reader.
- Corrected CP6's confidence: its dump establishes an unreadable packaged source in the texture hash path, not the cause/time of source invalidation. A synthetic source-release test cannot establish that missing causal chain. CP7 heap corruption remains unresolved and may involve CP6 itself.
- Reviewed current initialization, publication, upload callback, retirement and cleanup. No proven corrupting write/free found. Failed host symbols include the existing compute retirement functions; their presence does not establish that every release uses them.
- One guarded offline probe covers both captured BC7 layouts, nonuniform mip data, source release, allocation boundaries and cleanup. Passed without game/GPU. Probe/code fingerprints and limits are in ignored build/mod-adapt/gamebanana/lifetime-audit/manifest.json. This does not test actual render concurrency or reproduce the heap failure.
- Corrected documentation and the test's evidence comment only. No runtime patch, host replacement, game launch or new user test requested. Keep the failing host available for later capture. Detailed assessment: docs/MOD_RUNTIME_LIFETIME_AUDIT_2026-10-17.md.
- Recovery anchor: refs/checkpoints/mod-adapt/before-lifetime-audit. Other sessions' uncommitted runtime changes remain outside the resource commit.


## CP9 — startup heap corruption traced to mixed staged layouts (2026-10-17)

- Fresh first-chance dump from user session 20261017-145819-115532 at 14:59:45: formal unwind with exact host symbols and timestamp/image-size checked system images reaches RtlFreeHeap -> tex_release_packaged -> nvn_front_tex_read -> vt_get2. Ordinary pool texture 0x1d30a306680 has packaged=0 and packaged_copy=-1.
- Matching failed binary proves tex_ahead writes -1 at Tex+0xf0, while tex_release_packaged frees Tex+0xf0. Root cause is the ignored staging helper flattening runtime/r1src/nvn_vk.c over runtime/nvn_vk.c; its relative include escapes via the generated include directory into old build/work sources/header. CP6 changed the front-end layout only in the actual compiled code, despite all 52 objects being rebuilt.
- Added tracked build_resource_test_host.py, preserving runtime-relative paths and replacing obsolete generated input copies without writing through hardlinks. Resolve real compiler dependencies before linking; preserve manifest for the next captured session. Local legacy builder delegates to it. No runtime behavior or game inputs changed in this checkpoint.
- All 52 host objects rebuilt. Final binary prewarmer uses +0xf8, snapshot release +0xf0. Incremental rerun rebuilds zero host objects. Runtime snapshot remains retest-fixes (5fb77e4) with current resource front-end overrides, retaining existing parallel focus/retirement fixes for manual testing.
- Extended the existing native texture regression to invoke the actual staged Vulkan prewarmer. Forced old header reproduces pointer overwrite (explicit exit 86); matching header preserves ownership for both packaged and ordinary pool textures and passes ten-mip read/cleanup checks. Prior borrowed-source access-violation negative remains valid. No game/window/GPU session started.
- Evidence and binary disassembly: ignored build/mod-adapt/heap-investigation/. Recovery anchor: refs/checkpoints/mod-adapt/before-host-layout-fix. Single resource commit remains based directly on yuzu; unrelated working files preserved byte for byte.
- User should retry only test 02: title/menu + audio, Knuckles/Kirby -> playable match, then return/re-enter once. Actual gameplay and the original packaged-source failure remain pending; tests 03–09 stay paused.


## CP10 — user passes skin test, advance to single-slot voice (2026-10-17)

- User explicitly reports test 02 passed. Latest skin session 20261017-151522-213737 exits 0, has no captured fatal exceptions, and records unchanged original game inputs. Visual/interaction acceptance comes from the user; exit status alone is not treated as proof of every resource effect.
- Test 03 configuration confirmed without launching the game: only mario-voice enabled, volume 100, same host binary that passed test 02. Installed payload is vc_mario_c00.nus3audio plus six Mario _00 UI textures; no in-match model replacement.
- Next manual check is Mario c00 voice/UI, then c01 original voice after re-entry; observe missing audio, first-use failure and cross-slot leakage. No rebuild or redundant automated/game testing performed. Tests 03–09 remain unverified.
- Documentation-only checkpoint; resource changes remain one commit over yuzu and unrelated runtime working changes remain untouched.


## CP11 — user passes voice test, complete patch-group payload (2026-10-17)

- User explicitly reports test 03 passed. Latest voice session 20261017-152203-968911 exits 0 and records unchanged original game inputs. Tests 02 and 03 are now manually accepted; 04–09 remain pending.
- Verified test 04 enables only sora-soundtest, announcer-param, game-announcer, with volume 100 and the same accepted host. No host/runtime/backend rebuild or game session performed.
- Preflight caught an installation omission: game-announcer lacked its layout.arc despite that file being present in the downloaded ZIP. Verified download SHA-256 against manifest; copied the exact 14,763,152-byte mod payload to its original relative path under ignored packs. Confirmed original archive target exists and virtual archive reads match the restored mod file byte-for-byte. No original game file or generated game code changed.
- Focused backend preparation passes with 5 replacements, 4 patched inputs, 4 groups, zero diagnostics. Local checksum/read evidence: build/mod-adapt/gamebanana/test04-restored-layout.json. Third-party payload remains ignored and is not redistributed in Git.
- Next user test: Sora's five costume sound-test labels and selectable/playable entries; one short match for stretched skull end layout, replacement GAME! voice, and intact other/winner announcements. Parameter patch has no guaranteed obvious effect in an ordinary scene; do not demand repeated matches to prove it.
- Documentation checkpoint only in the resource commit; parallel runtime edits and accepted host binary remain unchanged. Recovery anchor: refs/checkpoints/mod-adapt/before-test04.


## CP12 — integrate lifecycle capture alongside test 4 (2026-10-17)

- User clarified that manual testing has advanced to item 4. Preserve passed skin/voice results and the existing patches session; an exit code 0 alone does not establish item 4's functional pass. No reset or forced replay of items 2/3.
- Integrated the independent opt-in lifecycle recorder/analyzer. Keep unrelated compute retirement/focus changes outside the owned resource commit. No Tex layout change and no game-file edit.
- Extended build_resource_test_host.py with separate destination/baseline options: verify every source against the validated host manifest, preserve nested paths, overlay explicit runtime observation sources, check real compiler header dependencies, and compile 53 host files into host-lifetime. Validated original c7806123… EXE is unchanged; diagnostic EXE SHA-256 is e25562d891c392a233e38d39697d592ccf1d2277e9360787aee80ef6645e7194.
- Added opt-in launcher support for a separate host, copied case-specific save and runs/lifetime/<case> evidence. Case 4 shares the original three-pack profile and 100 percent volume; its first diagnostic run seeds from the current case-4 save. Fatal capture and matching symbols apply to this diagnostic case too. Original numbered launchers remain unchanged.
- One native stream/analysis check passes. Existing staged Vulkan prewarmer/texture check passes against the new host; deliberate stale-layout negative detects the previous corruption mechanism. Trace-enabled real texture paths produce 60 valid records with no unmatched reads or false hazards. Compiler dependency and read-only launcher descriptions pass. No game/GPU started.
- Verified original host/04 entry/profile and all existing 2/3/4 save/session state remain unchanged. Evidence: ignored lifetime-integration/before.json, preservation-check.json, texture-check and the two launch descriptions. No claim of new in-game acceptance.
- Diagnostic launches also verify and snapshot the regular host's currently built backend DLL/manifest into the separate host; later item-4 backend fixes are followed without modifying the regular host. Read-only descriptions never synchronize files; sessions retain the actual backend fingerprint.
- Recovery anchor: refs/checkpoints/mod-adapt/before-lifetime-integration. User-facing next entry: 04-文字参数与播报补丁-生命周期诊断.cmd; use the existing item-4 checklist plus one return/re-entry if the short match completes.


## CP12 — preserve localized request classes and rebuild selected companion groups (2026-10-17)

- User reports animated infinite pre-game loading in test 04 session 20261017-153100-470424. Its final exit 0 comes after user closure and is not acceptance. Existing samples show waiting resource threads, completed shader jobs; total 2102 presented frames. No fresh post-stall full memory dump; exact pending file is not identified by those stacks.
- Found a concrete native contract violation in archive.rs: localized replacements were moved into the base group and FileInfo region/locale flags cleared while their directory continued scheduling regional companions. This affects message table, common narration audio + implicit bank, and the newly installed end layout. Standalone successful file read had missed this distinction.
- Backend source now preserves request class, rebuilds each affected active companion's contiguous physical data stream, and clones/retargets full InfoToData spans with their flags. Inactive languages and untouched neighboring files retain original content. Existing ordinary group path remains in place.
- Added one focused private-archive check (no game/window/GPU): old DLL fails the region-class assertion; fixed DLL passes independent us_en and zh_cn processes, three representative regional paths plus a five-locale fixture, native selected group and stream cursor, inactive language byte preservation, 121 neighboring resources per region, and companion bank's expected ID-only change. Markers stay outside live packs. The region-zero dummy can have a compression flag without a frame, so its stored bytes/metadata are compared without trying to decode it.
- Rust 7 unit checks and the existing native archive layout/sharing/stream integration pass. Actual patches profile prepares 5 replacements, 4 patched inputs, 7 groups without diagnostics; Chinese selected layout read matches installed mod. Skin and voice real-profile preparation also passes; prior user acceptance of those groups is not relabeled as a new-DLL playtest.
- Rebuilt only the Rust backend and replaced DLL + build fingerprint in the existing test host. Accepted EXE and all original game inputs unchanged; no frontend/renderer/game code edits. Parallel working runtime files preserved byte-for-byte.
- Before-state evidence, diagnosis and next launch configuration: ignored build/mod-adapt/gamebanana/bug04-loading/. Recovery anchor refs/checkpoints/mod-adapt/before-patches-loading-fix. Single resource commit remains based on yuzu.
- Next user step: retry the same test 04 launcher, first verify passing the formerly endless loading into title/menu, then check sound-test labels/playback and one end-of-match layout/voice. Actual recovery is pending; tests 05–09 remain paused.


## CP13 — retain standalone announcer bank identity (2026-10-17)

- User confirms test 04 now loads and renders the skull ending layout, but start/end narration is silent. Latest local run 20261017-162654-867744 has volume 100% and other audible output. Test 04 remains only partially accepted; later groups stay pending.
- Independent NUS3 section inspection proves merged common narration has 37 original TNIDs, only vc_narration_gameset sample replaced, other 36 samples unchanged. Matching original/virtual nus3bank instead differs at 0xbc: bank ID 4500 became 7420. Original narration bank/audio each have one path; Mario c00 bank/audio each have nine aliases.
- Local ARCropolis unshare.rs returns early for non-shared resources; only actually unshared banks receive the flag consumed by threads.rs for bank-ID reassignment. The adapter incorrectly renumbered every companion bank for an audio replacement.
- Backend archive.rs now resolves original bank sharing chains, includes both aliases and canonical sources, and allocates identities only to shared/added banks outside keep-shared rules. Standalone bank payloads are preserved. No frontend, renderer or game-source changes.
- Added one private integration test using the downloaded single-sample patch plus an isolated invalid Mario marker: old DLL fails standalone narration identity; new DLL preserves entire narration bank and all 37 track IDs, changes only GAME! sample, still isolates shared Mario c00 and preserves c01 bank/voice. No game/GPU startup; third-party bytes are not committed.
- Corrected CP12 fixture's overly broad assumption that the standalone companion bank should be renumbered. Localized regression now requires all 122 neighbors unchanged and passes us_en + zh_cn. Seven Rust unit tests pass. Actual patches profile's bank ID 4500, merged samples and installed skull layout match expectations.
- New backend DLL + fingerprint staged beside the same accepted host EXE. Original game input sizes/mtimes, EXE fingerprint and parallel runtime file bytes remain unchanged. Recovery anchor refs/checkpoints/mod-adapt/before-announcer-audio-fix; ignored evidence build/mod-adapt/gamebanana/bug04-audio/.
- User retest only test 04 narration: opening countdown/GO!, replaced end call, winner name. Actual audible recovery remains pending; resource work stays a single commit over yuzu.


## CP14 — user accepts patch group, advance to R.O.B. effects/motion (2026-10-17)

- User explicitly reports test 04 passed after the standalone narration-bank fix. Record manual acceptance of 02, 03, 04; do not infer an exit result for the latest 164230 session while result.json is absent.
- Next test 05 enables only rob-motion with volume 100 and the same accepted host/backend. Actual installed payload includes eight c00–c07 motion_list.yml files and ef_robot.eff. Local author description confirms PilotLampS/M/L and EyeFlash removal, and removal of effect_win3/effect_appealsl/effect_appealsr references. No agent game run or host/backend rebuild.
- Actual motion profile prepares successfully through the current backend. Test request is a short R.O.B. match with ordinary/special attacks, both side taunts, one victory animation, then change to a second color and re-enter. All eight installed colors intentionally remove the lamp; a second color is not a vanilla control. Do not grind every victory animation.
- Documentation-only checkpoint; tests 05–09 remain pending. Original game files and parallel runtime changes untouched. Third-party payload stays local and ignored, respecting the author's no-modpack-redistribution notice. Resource work remains a single commit over yuzu.


## CP15 — review accepted test 04 lifecycle evidence (2026-10-17)

- User accepts test 04 and keeps this task focused on lifecycle. Main resource testing is at test 05; no game/GPU run by this task.
- Analyze lifetime/patches/20261017-164311-378132: 69,639 records, no partial tail or unmatched intervals, 24,021 reads across five threads, 4,216 pool generations / 84 pool-address reinitializations, 1,375 successful page decommits. No recorded violation. Session has no result.json; do not invent its exit status.
- All 9,230 owned reads have matching original pool/packaged addresses. None follows observed original-source release or pool finalization, so successful play does not establish that snapshotting saved a released source or identify the original fault's owner.
- Close analyzer blind spot: snapshot cleanup while an owned read remains open, retaining the snapshot identity from read begin. Add coverage/post-source-release counters. Extend existing native contract check: old analyzer misses new overlap, current reports it; check passes. No invalid dereference or game/GPU run.
- Audit distinguishes synchronous Vulkan pool retirement, immediate upload-ring copies, unobserved protection changes and guest allocator sub-block recycling. No speculative ownership/synchronization patch or parallel heap fix.
- Prepare only next test 05 diagnostic entry using existing motion profile/backend-follow mechanism; read-only preflight passes. Original entries and diagnostic EXE intact. Evidence: gamebanana/lifetime-followup/accepted-patches/. Collect during planned test 05 without repeating accepted groups.
- Game input metadata and diagnostic host hash match session. Changes limited to analyzer, existing native check and lifecycle docs; resource work remains one commit over yuzu.


## CP16 — accept the existing-fix explanation and bound lifecycle work (2026-10-17)

- User completed the next diagnostic test and asks whether existing fixes explain the absence of violations. Treat this as a valid leading explanation; unresolved historical provenance is not evidence of a current defect.
- Review lifetime/motion/20261017-165724-059463: 315,651 records, 98,274 reads, 2,072 pool-address reinitializations and 10,134 page decommits; no recorded violation or unmatched interval. No result.json; do not infer exit status or independently assert resource-effect acceptance.
- Recheck retained upstream compute before/after results and actual texture/prewarmer controls. Compute retirement and mixed-layout build fixes have causal controls; snapshotting protects the tested source-loss path, while the original texture address's invalidator remains historically unproven.
- Retract only this task's unbuilt nu/permission/protocol observation edits. No new runtime behavior, diagnostic host or special manual retest. Preserve the parallel runtime/backend work.
- Keep offline analysis improvement: bounded 64 KiB candidate index replaces full-history scans on every release; retain exact range checks and finalized texture entries. Three real reports match the prior algorithm. Current large trace takes about 0.64 s; 102 range-oracle checks and existing native contract pass. Evidence: lifetime-followup/motion-review/verification.json.
- Close this investigation phase as stable in observed scenarios with historical texture attribution unresolved. Resume a specific lifecycle fault only on new symptoms or contradictory evidence. Original game input metadata and accepted diagnostic host hash remain unchanged; one resource commit over yuzu.


## CP17 — user accepts R.O.B.; support externally shared localized voice before Haze test (2026-10-17)

- User explicitly reports test 05 passed. Latest motion session 20261017-164927-451166 exits 0 with unchanged original inputs. Tests 02–05 have user acceptance; 06–09 remain pending.
- Test 06 preflight with the current backend caught a real failure before requesting play: Palutena c00 voice/bank FileInfos in directory 3722 have regional/shared flags and point to external folder 3336, outside local span 13130..13144. CP12 incorrectly rejected this legal sharing pattern.
- Also established that companion file_count includes borrowed entries: this real folder declares 16 files but stores only 10; copying 16 physical FileData rows reaches the following language. Backend now walks matching FileInfos, preserves unmodified borrowed links with zero-byte placeholders, materializes modified shared entries into the local selected group, and sets owned mapping bits. Complete inactive-language mapping chains remain intact.
- Added one focused private regression: old DLL fails opening the marker profile; fixed DLL proves owned localized voice/bank slots, correct native owned-file byte cursor, inactive language byte/metadata preservation, isolated bank ID, and unchanged c01/c07 voice/banks. Invalid marker stays outside live packs; no game/GPU startup.
- Existing normal archive integration, localized us_en/zh_cn, independent/shared sound-bank checks and seven Rust tests pass. Actual Haze preflight returns 47 replacements, 2 additions, 8 groups, zero diagnostics. All 49 plan assets read exactly, the c00 EFF is included in the c00 load group, and c01/c07 models/voice/banks remain original.
- Rebuilt/staged resource backend DLL + matching fingerprint only; accepted host EXE, original game inputs and parallel runtime working files unchanged. Recovery anchor refs/checkpoints/mod-adapt/before-haze-localized-fix; ignored evidence build/mod-adapt/gamebanana/bug06-shared-localized/. Resource changes remain a single commit over yuzu.
- Manual next step: original 06 launcher, Palutena c00 Haze versus optional c01 vanilla, ordinary/special attacks and taunts for Torna effects/voice, one death/respawn and victory, then color change/re-entry. Actual visual/audio effects pending user test.

- Original-mod integrity boundary verified: all 50 installed Haze files, including config.json, match the downloaded archive by SHA-256; archive itself matches its recorded manifest hash. Evidence: bug06-shared-localized/original-mod-integrity.json. No installed mod file was written; compatibility is provided by the backend virtual archive.

## CP18 — retain added costume effects across color changes (2026-10-17)

- User reports test 06 c00 appearance loads correctly, but restarting as vanilla after that match crashes; possible stutter remains uncertain. Do not infer full EFF/voice acceptance. Tests 02–05 retain user acceptance; 06–09 remain pending.
- Analyze effects/20261017-172151-124366 without starting the game. Matching native unwind and disassembly resolve TaskWorker2 null read to rd8 at game RVA 0x1dd02620, called by EFF-header reader 0x7103560410 in 0x710355f8f0. Guest x19=590711 and x14=0x2b71aadd82 identify the added ef_palutena_c00.eff; retained stack includes c01 model path and effect/fighter/palutena.
- Original backend only owns that EFF in c00. The effect registry enumerates actor-folder search entries when recreating the character; c01 has no load dependency for the added library. Backend now places added actor EFFs with an existing canonical vanilla cmn/effect library, retains that single owner via native costume-directory dependencies, and gives EFFs native 4096-byte allocation alignment. No game source/file, frontend or graphics-runtime change.
- Focused isolated EFF regression fails c01 availability on old DLL and passes on new DLL for all eight original colors, single owner, base EFF retention, allocation alignment and native bulk stream cursor; original c00/c01/c07 models/voice/banks stay intact. Generic bulk-check helper skips regional/localized selector-zero rows, which belong to companion streams; both old/new real Haze demonstrate the prior checker false positive. Existing normal archive and shared-localized regressions plus seven Rust unit tests pass.
- Real Haze preflight: 47 replacements, 2 additions, 9 groups, no diagnostics; all 49 assets read exactly, every original color reaches the added EFF through native dependencies. Vanilla common EFF and c01/c07 model/voice/bank bytes preserved. All 50 installed mod files still match original-archive hash evidence. Stage only backend DLL/PDB/build stamp beside unchanged accepted host EXE; original game input metadata unchanged. Recovery anchor refs/checkpoints/mod-adapt/before-haze-effect-reload; ignored evidence bug06-color-reload/.
- Stutter scope: 2366 battle frames, 59.3 FPS, present median 16.54 ms / p99 22.39 / max 189.02, three frames over 33.4 ms. Existing reports do not establish a resource/move cause. No speculative renderer/performance fix.
- Next manual test: c00 match -> c01 match -> c00 re-entry, verify vanilla voice/appearance and Haze effect/voice reload, note first-use stalls. Actual recovery pending user feedback; no extra special launcher or repeat of accepted tests. Preserve parallel working changes and keep resource work one commit over yuzu.

## CP19 — user accepts Haze; advance to stage resources (2026-10-17)

- User explicitly reports test 06 passed after CP18. Record manual acceptance of 02–06; 07–09 remain pending. Latest ordinary effects session 20261017-233423-996220 records exit 0, unchanged original inputs, CP18 backend stamp and accepted host EXE. Do not claim the earlier hitch cause is resolved.
- Next test 07 enables bell-island only, volume 100, same host/backend. Local author metadata identifies Super Bell Island over Yoshi's Story, with Omega/Battlefield support. Installed package includes normal/battle stage models, material/texture additions, UI and a stage-name xmsbt patch; it has no sound payload, so do not require a changed music track for acceptance.
- Offline preflight of the actual profile succeeds: 215 plan resources, 60 replacements, 155 additions, one patched input, 24 groups, zero diagnostics; all 214 direct assets match installed bytes and the message patch is readable. Verify 161 explicit new-dir-files memberships and native bulk stream layout. All 218 installed file hashes and original ARC size/mtime unchanged; no game/GPU run, code change, or rebuild.
- Manual next step: normal Yoshi's Story/Super Bell Island short match with edge/collision observation; enter Battlefield and Omega variants once each; switch to a vanilla stage and return. Check missing/black textures, wrong geometry, stalled loading, results and reload. Avoid repeating accepted groups or grinding all variants. Evidence: ignored gamebanana/stage-preflight/verification.json and next-launch.json.
- Documentation-only checkpoint; preserve parallel runtime/script/test changes and retain a single resource commit over yuzu.

## CP20 — user accepts stage; advance to music/movie streams (2026-10-17)

- User explicitly reports test 07 passed. Record manual acceptance of 02–07; 08–09 remain pending. Latest ordinary stage session 20261017-234408-483241 records exit 0 and unchanged original input metadata. Acceptance comes from user feedback, not inferred exit status alone.
- Next 08 original launcher enables tetris-bgm + kart-intro only, volume 100, same verified backend pair and accepted host. Local original song list maps Humoresque of a Little Dog to Tetris Game Boy and Magicant (New Remix) to Tetris Orchestral. Original Mario Kart Wii intro description notes slower movie loading on the author's system. No song-name database or extra BGM plugin in these packages.
- Offline actual-profile preflight: three streams, no diagnostics. Verify full bytes and beginning/middle/end partial reads through encoded native stream entries for c2_opening.webm (19,531,461 bytes), bgm_k14_mtr_magicant.nus3audio (1,728,448), bgm_k05_mtr_humoresqueofalittledog.nus3audio (1,407,736). Six installed file hashes and original ARC metadata unchanged. No game/GPU startup, code change or rebuild. Evidence: ignored gamebanana/streams-preflight/verification.json and next-launch.json.
- User test: opening movie playback/sound/synchronization and normal return; both original-name Mother tracks play their Tetris replacements, switching and re-entry work; if supported, use progress adjustment for middle/tail and note loop glitches. Runtime decoding, seeking, loops and movie return are pending user observation; offline bytes alone do not prove them.
- Documentation-only checkpoint; preserve parallel working changes and retain one clean resource commit above yuzu. No repeats of accepted groups or long playback stress tests.

## CP21 — keep music stream offsets addressable by the native consumer (2026-10-18)

- User reports test 08 opening replaced normally, but both replaced BGM tracks never start (stationary progress and silence); original tracks play. Record movie acceptance only, keep 08 music and 09 pending. Session streams/20261017-235440-281946 exits 0 with original input metadata unchanged; no playback acceptance inferred from that.
- Read native 13.0.4 logic: stream lookup 0x7103541200 supplies 64-bit offsets, music path 0x710373c740 loads one and 0x710373c958 narrows it to u32. Old adapter places Magicant at 0x41a41fb20, whose truncated 0x1a41fb20 reads unrelated original archive bytes. Video's path preserves 64 bits. This explains why a 64-bit API byte check passed while gameplay BGM fails; no codec or source-mod rewrite is justified.
- Backend-only virtual layout fix: place replacement stream segments before the original stream section, retain a read-only mapped original suffix, relocate all original stream-data offsets and header file/shared section bases. Original patch/copy reads keep the physical source base; appended normal/localized groups use the new virtual base. Native 32-bit capacity bounds are checked before publishing. No game source, generated code, frontend, runtime or installed-mod changes.
- One focused private integration test reproduces native narrowing: old DLL fails actual Magicant header read, new passes full two-BGM/movie bytes and beginning/middle/end reads; 2887 untouched stream hash mappings retain size/flags, with sampled original stream tail bytes plus original model/region voice bytes intact. Existing mixed resource/stream integration and seven Rust tests pass.
- Real combined offline check passes all 310 direct assets, 12 generated patch targets, 3 streams, 50 groups, native bulk layout, Haze dependency graph for all original colors, and vanilla c01 model/voice. Only pre-existing malformed Knuckles XMSBT warning remains; never edit its source. Actual combined gameplay still pending test 09.
- All six installed music/movie package files match original extraction; both download SHA-256 values match manifest. Stage only new backend DLL/PDB/build stamp beside unchanged accepted host EXE; original game input metadata unchanged. No game/GPU or audio playback by agent. Evidence: ignored gamebanana/bug08-music-streams/; recovery refs/checkpoints/mod-adapt/before-bgm-stream-offset-fix.
- Manual next step: original 08 launcher, both original-name Mother tracks must start progress and play Tetris replacements; switch between them and an original control, leave/re-enter. No dedicated full movie repeat or accepted-group reruns. Actual audible recovery pending user feedback; preserve parallel work and one clean resource commit from the accepted yuzu baseline. During finalization yuzu advances to 898bd615 (parallel packaged-texture port); keep this tested commit parent f877d972 and the accepted host rather than silently changing the retest baseline.

## CP22 — user accepts music streams; advance to combined acceptance (2026-10-18)

- User explicitly reports test 08 passed after CP21. Record manual acceptance of 02–08; 09 remains pending. Recent ordinary streams session 20261018-000942-935531 exits 0 with original input metadata unchanged, accepted CP21 backend source/DLL hashes and accepted host EXE; use user feedback for audible acceptance.
- Original 09 launcher enables all eleven packages at volume 100 using the same host/backend. Verify staged DLL, build stamp and source fingerprint match accepted 08, so reuse CP21 full actual combined offline check (322 plan targets, 310 exact direct contents, 12 generated patch targets, 3 streams, 50 groups, native bulk layout and Haze dependency graph) instead of rerunning it.
- Focused en-US/us_en combined preparation succeeds; assert Super Bell Island plus all five Sora costume names coexist in the merged message file and three stream headers remain readable within native 32-bit ranges. Only existing Knuckles malformed XMSBT warning remains; no source mod repair. No code change, rebuild or game/GPU startup.
- Ignored local English launcher uses existing play_resource_test.py combined --language en-US; only overrides process language/region, keeps the same enabled package list/volume and original 09 default. Evidence: combined-preflight/next-launch.json, english-launch.json and english-verification.json.
- Manual request: short Knuckles-versus-Haze match on Super Bell Island through skull/announcer results, then Mario c00 versus vanilla Palutena c01 on an original stage to cover voice isolation and reload; one music sample plus Sora/stage names; separate English menu-name check without another full match. No full individual-test repeat. Actual combined acceptance remains pending and does not establish every ARCropolis mod works.
- Documentation-only checkpoint and ignored helper; preserve parallel working changes and keep the single resource commit from the accepted yuzu baseline. Do not move to the concurrently updated yuzu or rebuild the accepted host during this manual acceptance step.

## CP23 — user accepts combined test; close this resource-package acceptance round (2026-10-18)

- User explicitly reports test 09 passed. Record user acceptance of 02–09, including all eleven packages together. Latest combined/20261018-001449-575360 exits 0 and retains original input metadata; acceptance remains the user's explicit feedback. No new game/GPU launch, regression rerun, source change or rebuild for closeout.
- Verify current backend source fingerprint f3234f17… matches staged/tested DLL 0f754d51…, and accepted host EXE c7806123… matches combined launch. Preserve the exact accepted code/build while updating documentation. Evidence summary: ignored gamebanana/acceptance-final/verification.json.
- Update MOD_RESOURCES.md and manual test guide with completed package coverage and final entry, preserving historical failure/retest notes. Explain the validated common-EFF dependency and music stream virtual prefix, native stream address limit, and existing malformed Knuckles XMSBT diagnostic.
- Bound the result to this package set and user feedback: native Switch plugin.nro/Skyline execution and added-character game logic remain unsupported; full ecosystem/long-duration/all-region validation not claimed. English merged names/stream lookup were checked offline; no separate English process record is asserted from the combined session without language overrides.
- Retain one resource commit from accepted base f877d972, separate recovery refs, original game and mod files, ignored third-party downloads, and every parallel working change. Current yuzu has advanced independently; do not silently rebuild or alter the accepted manual-test baseline during closeout.


## CP24 — optional yuzu integration and Console management (2026-10-18)

- Integrate the accepted resource commit into yuzu 41b16e8 in a separate worktree. Preserve yuzu's packaged-texture ownership and deferred-compute retirement fixes, plus the original checkout's unrelated uncommitted work. No game-derived source or original game/mod inputs are edited.
- Resource packages are opt-in (`--with-mods`); the default / `--without-mods` host uses filesystem passthrough and an explicit feature-disabled management response. The build resume state and host object flags detect feature changes. Existing PC Hook DLL/fighter interfaces stay independent.
- Host binaries carry a hash-checked build record. Packaging follows the actual feature, excludes stale backend files from disabled packages, and includes the DLL, fingerprint, licenses and optional PDB only when enabled. Install root is `<package>/mods/<name>/`, beside `game/`; profiles use portable paths. No third-party mods are automatically redistributed.
- User additionally authorizes a Console Mods page. Reuse existing typography, colors, slanted controls, navigation transitions and responsive/reduced-motion behavior. Detect support from the host record/hash and package manifest. Label resource categories/characters/costumes, plugin dependencies and file conflicts. Manage aliases, enable flags, virtual costume mappings, ZIP/folder imports, Explorer access and reversible deletion into `.trash`. Block mutation while the game runs, sharing a lock with game startup. Keep broken packages manageable.
- Slot changes rewrite virtual targets and readable path rules, preserving disk paths and bytes; reject ambiguous hashes, executable dependencies and colliding costume targets. Extra-slot game logic remains outside the compatibility claim.
- User notices missing font glyphs. Expand static subset input to backend strings, and add the same font's complete renamed OFL WOFF2 fallback for dynamic mod names and aliases. Browser platform-font inspection confirms the four characters in 纳克鲁斯 use custom project fonts, including 纳 from the complete face.
- Focused validation: nine resource-backend unit tests, five optional-build/packaging checks, two Console management tests including the full import/alias/enable/slot/delete/ZIP and malformed-profile recovery path. Browser checks cover dark/light/narrow layouts, alias/slot event handlers and disabled-package controls. No game is launched. Offline artifacts and package verification are kept in ignored `build/mod-integration/`.
- Full directory packages are built with and without the extension. ZIP packaging clamps pre-1980 dependency-license timestamps and preserves the mod folder layout while omitting data.arc; a focused regression covers that exact case.
