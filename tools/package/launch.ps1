<#
  ssbu-recomp test package launcher (copied into every package by scripts/package.py; Windows PowerShell 5.1, no Python).

  Start.bat     -Mode play      play; every run gets logs\<session>\ and, at the end, reports\<session>.zip
  SmokeTest.bat -Mode smoke     boot check: the game runs for smoke_seconds (launcher.ini, default 90) from the opening movie
                                on, with pictures of the screen in the session; PASS = no crash, trap, device loss, freeze,
                                refused script module or missing RomFS file, the movie started, the whole time ran. -Match: a scripted fight instead (tests\seq_*.txt)
  Replay.bat    -Mode replay -Report <zip or folder>   the run of a report again on this PC: same build (checked), same
                                save, settings, random seed and the recorded pad input of player 1
  Dump.bat      -Mode dump      minidump of the running game (for a hang the freeze watchdog does not see)

  A session folder holds: game.log (all output of the game), session.json (build, settings, environment, result),
  system.txt (OS, CPU, GPU and driver, Vulkan, displays, controllers), preflight.txt, input.rec (pad input of player 1),
  save_before.zip + config_before\ (the state the run started from), boot_trace.txt, fight_hash.txt (per fight frame),
  ssbu_native_crash.txt / _freeze.txt / *.dmp (when it happened), events.txt (Windows errors, driver resets, WHEA), note.txt.
#>
param(
    [ValidateSet('play', 'smoke', 'replay', 'dump')][string]$Mode = 'play',
    [string]$Report = '',
    [int]$Seconds = -1,
    [switch]$Verify,
    [switch]$Force,
    [switch]$LocalConfig,
    [switch]$NoPause,
    [switch]$Match
)
$ErrorActionPreference = 'Continue'
$Root = Split-Path -Parent $PSScriptRoot
$GameDir = Join-Path $Root 'game'
$Exe = Join-Path $GameDir 'ssbu_native.exe'
$Utf8 = New-Object System.Text.UTF8Encoding $false
$ExitMeaning = @{
    0 = 'normal exit'; 1 = 'load failure (or the process was ended from outside)'; 2 = 'data.arc not found, or a guest trap'
    3 = 'untranslated instruction'; 4 = 'host exception (crash; see ssbu_native_crash.txt and the .dmp)'
    5 = 'Vulkan fatal error / no Vulkan driver / SSBU_STRICT stop'; 6 = 'Vulkan set-up failure (e.g. ssbu_glslang.dll)'; 7 = 'main.npdm missing'
    8 = 'mod resource load failure; see the preceding mods: error and disable the failing package'
    -1073741819 = '0xC0000005 access violation outside the crash handler'; -1073740791 = '0xC0000409 fast fail / stack buffer overrun'
    -1073741571 = '0xC00000FD stack overflow'; -1073741515 = '0xC0000135 a DLL was not found'; -1073741701 = '0xC000007B bad image (32/64-bit DLL mix)'
    -1073741510 = '0xC000013A ended with Ctrl+C / console closed'; -1073741795 = '0xC000001D illegal instruction (CPU feature missing)'
}

function Say([string]$text, [string]$color = 'Gray') { Write-Host $text -ForegroundColor $color }
function WriteText([string]$path, [string]$text) { [IO.File]::WriteAllText($path, $text, $Utf8) }
function AddText([string]$path, [string]$text) { [IO.File]::AppendAllText($path, $text + "`r`n", $Utf8) }
function TryRun([scriptblock]$block, [string]$what) {
    try { & $block } catch { "($what failed: $($_.Exception.Message))" }
}

function Read-Ini([string]$path) {
    $ini = @{ run = [ordered]@{}; env = [ordered]@{} }
    if (-not (Test-Path -LiteralPath $path)) { return $ini }
    $section = 'run'
    foreach ($line in [IO.File]::ReadAllLines($path, [Text.Encoding]::UTF8)) {
        $t = $line.Trim()
        if (-not $t -or $t.StartsWith(';') -or $t.StartsWith('#')) { continue }
        if ($t -match '^\[(.+)\]$') { $section = $matches[1].Trim().ToLower(); if (-not $ini.Contains($section)) { $ini[$section] = [ordered]@{} }; continue }
        $eq = $t.IndexOf('=')
        if ($eq -lt 1) { continue }
        $ini[$section][$t.Substring(0, $eq).Trim()] = $t.Substring($eq + 1).Trim()
    }
    return $ini
}
function Setting([string]$name, [string]$default) {
    $v = $Ini.run[$name]
    if ($null -eq $v -or $v -eq '') { return $default }
    return $v
}
function Full([string]$p) {
    if (-not $p) { return $p }
    if (-not [IO.Path]::IsPathRooted($p)) { $p = Join-Path $Root $p }
    return [IO.Path]::GetFullPath($p)
}

$Manifest = $null
$ManifestPath = Join-Path $Root 'manifest.json'
if (Test-Path -LiteralPath $ManifestPath) { $Manifest = [IO.File]::ReadAllText($ManifestPath, [Text.Encoding]::UTF8) | ConvertFrom-Json }
$BuildId = if ($Manifest) { $Manifest.build_id } else { 'unknown' }
$Ini = Read-Ini (Join-Path $PSScriptRoot 'launcher.ini')

# ------------------------------------------------------------------ the package's own files
function Test-Package([bool]$hash) {
    $out = New-Object System.Collections.Generic.List[string]
    $bad = 0
    if (-not $Manifest) { $out.Add('manifest.json missing: the package cannot be checked'); return @{ text = $out; bad = 1 } }
    foreach ($p in $Manifest.files.PSObject.Properties) {
        $f = Join-Path $Root ($p.Name -replace '/', '\')
        $want = $p.Value
        if (-not (Test-Path -LiteralPath $f)) { $out.Add("MISSING  $($p.Name)"); $bad++; continue }
        $len = (Get-Item -LiteralPath $f).Length
        if ($len -ne [int64]$want.size) { $out.Add("SIZE     $($p.Name): $len bytes, expected $($want.size)"); $bad++; continue }
        if ($hash -and $want.sha256) {
            $h = (Get-FileHash -LiteralPath $f -Algorithm SHA256).Hash.ToLower()
            if ($h -ne $want.sha256) { $out.Add("CONTENT  $($p.Name): sha256 differs"); $bad++ }
        }
    }
    $out.Add(("{0} files of the manifest checked ({1}): {2} problem(s)" -f @($Manifest.files.PSObject.Properties).Count, $(if ($hash) { 'size and sha256' } else { 'size; -Verify also compares sha256' }), $bad))
    return @{ text = $out; bad = $bad }
}

# ------------------------------------------------------------------ this PC
function Get-SystemText {
    $s = New-Object System.Text.StringBuilder
    $add = { param($t) foreach ($x in @($t)) { [void]$s.AppendLine([string]$x) } }
    & $add "collected $(Get-Date -Format 'yyyy-MM-dd HH:mm:ss zzz') on $env:COMPUTERNAME by $env:USERNAME"
    & $add (TryRun {
        $os = Get-CimInstance Win32_OperatingSystem
        $cv = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'
        "OS: {0} {1} (build {2}.{3}, {4}), {5}; memory {6:N1} GB, free {7:N1} GB" -f $os.Caption, $cv.DisplayVersion, $os.BuildNumber, $cv.UBR, $os.OSArchitecture, ($os.MUILanguages -join ','), ($os.TotalVisibleMemorySize / 1MB), ($os.FreePhysicalMemory / 1MB)
    } 'OS')
    & $add (TryRun {
        $cs = Get-CimInstance Win32_ComputerSystem
        "machine: {0} {1}, {2:N1} GB installed" -f $cs.Manufacturer, $cs.Model, ($cs.TotalPhysicalMemory / 1GB)
    } 'machine')
    & $add (TryRun {
        Get-CimInstance Win32_Processor | ForEach-Object { "CPU: {0}, {1} cores / {2} logical processors, {3} MHz" -f $_.Name.Trim(), $_.NumberOfCores, $_.NumberOfLogicalProcessors, $_.MaxClockSpeed }
    } 'CPU')
    & $add (TryRun {
        Get-CimInstance Win32_VideoController | ForEach-Object {
            $date = if ($_.DriverDate) { $_.DriverDate.ToString('yyyy-MM-dd') } else { '?' }
            "GPU: {0}; driver {1} ({2}); {3}, {4} Hz; {5}" -f $_.Name, $_.DriverVersion, $date, $_.VideoModeDescription, $_.CurrentRefreshRate, $_.PNPDeviceID
        }
    } 'GPU')
    & $add (TryRun {
        Add-Type -AssemblyName System.Windows.Forms
        [System.Windows.Forms.Screen]::AllScreens | ForEach-Object { "display: {0} {1}x{2} at {3},{4}{5}" -f $_.DeviceName, $_.Bounds.Width, $_.Bounds.Height, $_.Bounds.X, $_.Bounds.Y, $(if ($_.Primary) { ' (primary)' } else { '' }) }
    } 'displays')
    & $add (TryRun {
        $dll = Join-Path $env:SystemRoot 'System32\vulkan-1.dll'
        if (Test-Path $dll) { "Vulkan loader: {0}" -f (Get-Item $dll).VersionInfo.FileVersion } else { 'Vulkan loader: vulkan-1.dll NOT FOUND (no Vulkan driver)' }
        foreach ($k in 'HKLM:\SOFTWARE\Khronos\Vulkan\Drivers', 'HKLM:\SOFTWARE\Khronos\Vulkan\ImplicitLayers') {
            if (Test-Path $k) { (Get-Item $k).Property | ForEach-Object { "  {0}: {1}" -f (Split-Path $k -Leaf), $_ } }
        }
    } 'Vulkan')
    & $add (TryRun {
        $vi = Get-Command vulkaninfo.exe -ErrorAction SilentlyContinue
        if (-not $vi) { $vi = Get-ChildItem 'C:\VulkanSDK\*\Bin\vulkaninfoSDK.exe', 'C:\VulkanSDK\*\Bin\vulkaninfo.exe' -ErrorAction SilentlyContinue | Select-Object -Last 1 }
        if ($vi) {
            $p = if ($vi.Source) { $vi.Source } else { $vi.FullName }
            $out = @(& $p --summary 2>&1 | ForEach-Object { "$_" })
            $dev = [Array]::IndexOf($out, ($out | Where-Object { $_ -match '^Devices:' } | Select-Object -First 1))
            'vulkaninfo --summary (instance version and devices):'
            $out | Where-Object { $_ -match 'Vulkan Instance Version' } | ForEach-Object { "  $_" }
            if ($dev -ge 0) { $out[$dev..($out.Count - 1)] | Select-Object -First 120 | ForEach-Object { "  $_" } }
        }
    } 'vulkaninfo')
    & $add (TryRun {
        $smi = Get-Command nvidia-smi.exe -ErrorAction SilentlyContinue
        if ($smi) { 'nvidia-smi: ' + ((& $smi.Source --query-gpu=name,driver_version,memory.total,pstate,temperature.gpu --format=csv,noheader 2>&1) -join ' | ') }
    } 'nvidia-smi')
    & $add (TryRun {
        Get-CimInstance Win32_PnPEntity -Filter "PNPClass='HIDClass' OR PNPClass='XnaComposite' OR PNPClass='XboxComposite'" |
            Where-Object { $_.Name -match 'game|controller|gamepad|xbox|joystick|dualsense|dualshock|手柄|游戏' } | ForEach-Object { "controller: {0} ({1}) {2}" -f $_.Name, $_.Status, $_.DeviceID }
    } 'controllers')
    & $add (TryRun { 'power: ' + ((powercfg /getactivescheme) -join ' ') } 'power plan')
    & $add (TryRun { "locale: {0}, UI {1}, time zone {2}" -f (Get-Culture).Name, (Get-UICulture).Name, [TimeZoneInfo]::Local.Id } 'locale')
    & $add (TryRun {
        $d = Get-PSDrive ($Root.Substring(0, 1)); "disk of the package: {0}: {1:N1} GB free of {2:N1} GB" -f $d.Name, ($d.Free / 1GB), (($d.Used + $d.Free) / 1GB)
    } 'disk')
    return $s.ToString()
}

function Test-Preflight([string]$romfs) {
    $lines = New-Object System.Collections.Generic.List[string]
    $fatal = New-Object System.Collections.Generic.List[string]
    if (-not (Test-Path -LiteralPath $Exe)) { $fatal.Add("game\ssbu_native.exe not found ($Exe)") }
    if (-not (Test-Path -LiteralPath (Join-Path $romfs 'data.arc'))) { $fatal.Add("data.arc not found in $romfs (set romfs = in launcher\launcher.ini)") }
    else {
        $len = (Get-Item -LiteralPath (Join-Path $romfs 'data.arc')).Length
        $lines.Add("data.arc: $(Join-Path $romfs 'data.arc'), $len bytes")
        if ($Manifest -and $Manifest.romfs -and [int64]$Manifest.romfs.size -ne $len) { $lines.Add("WARNING: data.arc has $len bytes, the build was made with $($Manifest.romfs.size) (another game version or a damaged copy)") }
    }
    # the rest of the RomFS: .nrr\*.nrr list the script modules the game may load; without them every module is refused
    # and the game waits on the loading screen before a fight (the first packages of 2026-10-15 lacked them)
    $nrrWant = @()
    if ($Manifest -and $Manifest.romfs -and $Manifest.romfs.other_files) { $nrrWant = @($Manifest.romfs.other_files | Where-Object { $_ -like '.nrr/*' }) }
    $nrrDir = Join-Path $romfs '.nrr'
    $nrrHave = @(Get-ChildItem -LiteralPath $nrrDir -Filter '*.nrr' -Force -ErrorAction SilentlyContinue)
    if (-not $nrrHave.Count) { $fatal.Add("$nrrDir\*.nrr not found: the game would refuse every script module and stop on the loading screen before a fight. Copy the .nrr folder of the RomFS dump (it sits beside data.arc)") }
    else {
        $missing = @($nrrWant | Where-Object { -not (Test-Path -LiteralPath (Join-Path $romfs ($_ -replace '/', '\'))) })
        if ($missing.Count) { $fatal.Add("RomFS incomplete, missing: $($missing -join ', ') (copy the whole .nrr folder of the dump)") }
        else { $lines.Add("RomFS .nrr: $($nrrHave.Count) files") }
    }
    # Windows parts the package cannot carry: Media Foundation (the exe imports it: without it, Windows N, it does not start),
    # and the Store codecs Media Foundation decodes the movies with (VP9 Video Extensions; Vorbis sound: Web Media Extensions)
    if (-not (Test-Path (Join-Path $env:SystemRoot 'System32\mfplat.dll'))) { $fatal.Add('Media Foundation is missing (mfplat.dll; Windows N without the Media Feature Pack): install the Media Feature Pack') }
    if (-not (Test-Path (Join-Path $env:SystemRoot 'System32\icu.dll'))) { $lines.Add('WARNING: icu.dll missing (Windows 10 older than 1903): time zone and date handling of the game may be wrong') }
    try {
        $vp9 = Get-AppxPackage -Name 'Microsoft.VP9VideoExtensions' -ErrorAction Stop
        $wme = Get-AppxPackage -Name 'Microsoft.WebMediaExtensions' -ErrorAction Stop
        if (-not $vp9) { $lines.Add('WARNING: "VP9 Video Extensions" (Microsoft Store) not installed: the game''s movies (VP9) cannot be decoded') }
        if (-not $wme) { $lines.Add('WARNING: "Web Media Extensions" (Microsoft Store) not installed: the movies'' Vorbis sound may not be decoded') }
        $lines.Add("codecs: VP9 Video Extensions $(if ($vp9) { $vp9.Version } else { 'missing' }), Web Media Extensions $(if ($wme) { $wme.Version } else { 'missing' })")
    } catch { $lines.Add("codecs: not checked ($($_.Exception.Message))") }
    if (-not (Test-Path (Join-Path $env:SystemRoot 'System32\vulkan-1.dll'))) { $lines.Add('WARNING: no Vulkan driver (vulkan-1.dll); the game needs Vulkan 1.3 or renderer = gl in config\graphics.ini') }
    if ($Root -match '[^\x20-\x7E]') { $lines.Add("WARNING: the package path has non-ASCII characters ($Root); the game opens files with ANSI paths, characters outside the system code page break it") }
    if ($Root.Length -gt 120) { $lines.Add("WARNING: the package path is long ($($Root.Length) characters); Windows' 260-character limit can be reached in the cache") }
    try { $d = Get-PSDrive ($Root.Substring(0, 1)); if ($d.Free -lt 6GB) { $lines.Add(("WARNING: {0:N1} GB free on {1}: the shader caches of a first run need up to about 4 GB" -f ($d.Free / 1GB), $d.Name)) } } catch {}
    $pk = Test-Package $Verify.IsPresent
    foreach ($l in $pk.text) { $lines.Add("package: $l") }
    if ($pk.bad) { $lines.Add('WARNING: the package is incomplete or damaged (see the lines above): copy it again') }
    return @{ lines = $lines; fatal = $fatal; package_bad = $pk.bad }
}

# ------------------------------------------------------------------ after the run
function Get-Events([datetime]$since) {
    $s = New-Object System.Text.StringBuilder
    $emit = {
        param($e)
        $msg = ($e.Message -replace '\s+', ' ')
        if ($msg.Length -gt 600) { $msg = $msg.Substring(0, 600) + '...' }
        [void]$s.AppendLine(("{0}  {1}  {2} #{3} {4}: {5}" -f $e.TimeCreated.ToString('HH:mm:ss'), $e.LogName, $e.ProviderName, $e.Id, $e.LevelDisplayName, $msg))
    }
    try { Get-WinEvent -FilterHashtable @{ LogName = 'Application'; StartTime = $since; Id = 1000, 1001, 1002, 1005 } -ErrorAction Stop | Where-Object { $_.Message -match 'ssbu' } | ForEach-Object { & $emit $_ } } catch {}
    try { Get-WinEvent -FilterHashtable @{ LogName = 'System'; StartTime = $since; Level = 1, 2, 3 } -MaxEvents 200 -ErrorAction Stop | ForEach-Object { & $emit $_ } } catch {}
    if ($s.Length -eq 0) { return "no Windows errors or warnings since $($since.ToString('HH:mm:ss')) (Application: application errors of ssbu; System: all errors and warnings, e.g. Display 4101 = GPU driver reset, WHEA-Logger = hardware errors)" }
    return $s.ToString()
}

function Get-Verdict([int]$code, [string]$log, [string]$session) {
    $found = [ordered]@{}
    $markers = [ordered]@{
        'mod_resources' = 'MOD RESOURCE ERROR'
        'crash' = 'HOST EXCEPTION'; 'guest_trap' = 'GUEST TRAP'; 'untranslated' = 'UNTRANSLATED INSTRUCTION'
        'device_lost' = 'nvn_vk_lost|VK_ERROR_DEVICE_LOST'; 'freeze' = 'NO FRAME PRESENTED FOR'; 'freeze_recovered' = 'frames reach the window again'
        'debug_end' = 'debug end: \d+ fight frames reached'; 'movie' = 'input anchor 0 at poll'; 'time_limit' = 'still running after \d+ s with'; 'vulkan_fatal' = 'nvn_vk: FATAL: |nvn_vk: .+ failed: VK_ERROR'
    }
    $markers['nro_refused'] = 'in no registered NRR: refused'              # a script module the game may not load: it waits for ever
    if (Test-Path -LiteralPath $log) {
        foreach ($k in $markers.Keys) {
            $m = Select-String -LiteralPath $log -Pattern $markers[$k] -List -ErrorAction SilentlyContinue
            if ($m) { $found[$k] = $m.Line.Trim() }
        }
    }
    $trace = Join-Path $session 'boot_trace.txt'                           # a file of the game's own data that is not there
    if (Test-Path -LiteralPath $trace) {
        $m = @(Select-String -LiteralPath $trace -Pattern 'fs open "rom:/[^"]*" mode \d+ -> NOT FOUND' -ErrorAction SilentlyContinue | Select-Object -First 5)
        if ($m.Count) { $found['rom_missing'] = ($m | ForEach-Object { $_.Line.Trim() }) -join ' | ' }
    }
    $dumps = @(Get-ChildItem -LiteralPath $session -Filter '*.dmp' -ErrorAction SilentlyContinue)
    $result = 'ok'
    if ($code -ne 0) { $result = 'error' }
    foreach ($k in 'crash', 'guest_trap', 'untranslated', 'device_lost', 'vulkan_fatal', 'nro_refused', 'rom_missing') { if ($found.Contains($k)) { $result = 'error' } }
    if ($found.Contains('freeze') -and $result -eq 'ok') { $result = 'warning' }
    if (@($dumps | Where-Object { $_.Name -like '*_manual_*' }).Count -and $result -eq 'ok') { $found['manual_dump'] = 'Dump.bat was used: the tester saw a problem'; $result = 'warning' }
    return @{ result = $result; markers = $found; dumps = @($dumps | ForEach-Object { $_.Name }) }
}

function Compare-FightHash([string]$a, [string]$b) {
    if (-not (Test-Path -LiteralPath $a) -or -not (Test-Path -LiteralPath $b)) { return 'fight hash: not compared (one of the runs has no fight_hash.txt)' }
    $ra = New-Object IO.StreamReader($a); $rb = New-Object IO.StreamReader($b)
    try {
        $n = 0
        while ($true) {
            $la = $ra.ReadLine(); $lb = $rb.ReadLine(); $n++
            if ($null -eq $la -and $null -eq $lb) { if ($n -eq 1) { return 'fight hash: no fight in either run' }; return "fight hash: IDENTICAL, $($n - 1) lines (every fight frame of every fighter the same as in the original run)" }
            if ($null -eq $la -or $null -eq $lb) { return "fight hash: identical for $($n - 1) lines, then the $(if ($null -eq $la) { 'original' } else { 'replay' }) run ends" }
            if ($la -ne $lb) { return "fight hash: FIRST DIFFERENCE at line $n`r`n  original: $la`r`n  replay:   $lb" }
        }
    } finally { $ra.Close(); $rb.Close() }
}

function Trim-Sessions([string]$logs, [int]$keep) {
    $dirs = @(Get-ChildItem -LiteralPath $logs -Directory -ErrorAction SilentlyContinue | Where-Object { -not (Test-Path -LiteralPath (Join-Path $_.FullName 'running.txt')) } | Sort-Object Name)
    if ($dirs.Count -le $keep) { return }
    $dirs | Select-Object -First ($dirs.Count - $keep) | ForEach-Object { Remove-Item -LiteralPath $_.FullName -Recurse -Force -ErrorAction SilentlyContinue }
}

function Save-Snapshot([string]$userdata, [string]$zip, [int]$maxMb) {
    $save = Join-Path $userdata 'save'
    if (-not (Test-Path -LiteralPath $save)) { return 'save_before: no save yet (first start)' }
    $files = @(Get-ChildItem -LiteralPath $save -Recurse -File -ErrorAction SilentlyContinue)
    $bytes = ($files | Measure-Object Length -Sum).Sum
    if ($bytes -gt $maxMb * 1MB) { return ("save_before: not taken, the save is {0:N0} MB (save_snapshot_max_mb = {1})" -f ($bytes / 1MB), $maxMb) }
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $tmp = Join-Path ([IO.Path]::GetTempPath()) ("ssbu_save_" + [guid]::NewGuid().ToString('N'))
    New-Item -ItemType Directory -Path (Join-Path $tmp 'save') | Out-Null
    Get-ChildItem -LiteralPath $save -Force | Copy-Item -Destination (Join-Path $tmp 'save') -Recurse -Force
    [IO.Compression.ZipFile]::CreateFromDirectory($tmp, $zip, [IO.Compression.CompressionLevel]::Optimal, $false)
    Remove-Item -LiteralPath $tmp -Recurse -Force
    return ("save_before: {0} files, {1:N1} MB -> save_before.zip" -f $files.Count, ($bytes / 1MB))
}

# ------------------------------------------------------------------ Dump.bat: minidump of the running game
if ($Mode -eq 'dump') {
    $procs = @(Get-Process ssbu_native -ErrorAction SilentlyContinue)
    if (-not $procs) { Say '没有正在运行的 ssbu_native.exe。' Yellow; exit 1 }
    Add-Type -TypeDefinition @'
using System; using System.Runtime.InteropServices; using Microsoft.Win32.SafeHandles;
public static class SsbuDump {
    [DllImport("dbghelp.dll", SetLastError = true)]
    public static extern bool MiniDumpWriteDump(IntPtr process, uint pid, SafeFileHandle file, uint type, IntPtr exception, IntPtr user, IntPtr callback);
}
'@
    foreach ($p in $procs) {
        $target = $null
        Get-ChildItem -LiteralPath (Join-Path $Root 'logs') -Directory -ErrorAction SilentlyContinue | Sort-Object Name -Descending | ForEach-Object {
            $r = Join-Path $_.FullName 'running.txt'
            if (-not $target -and (Test-Path -LiteralPath $r) -and ((Get-Content -LiteralPath $r -TotalCount 1) -eq "$($p.Id)")) { $target = $_.FullName }
        }
        if (-not $target) { $target = Join-Path $Root 'logs'; New-Item -ItemType Directory -Force -Path $target | Out-Null }
        $file = Join-Path $target ("ssbu_native_manual_{0}.dmp" -f (Get-Date -Format 'yyyyMMdd-HHmmss'))
        $fs = [IO.File]::Create($file)
        try { $ok = [SsbuDump]::MiniDumpWriteDump($p.Handle, [uint32]$p.Id, $fs.SafeFileHandle, 0x1164, [IntPtr]::Zero, [IntPtr]::Zero, [IntPtr]::Zero) } finally { $fs.Close() }
        if ($ok) {
            AddText (Join-Path $target 'note.txt') ("{0}: manual minidump (Dump.bat) of process {1}: {2}" -f (Get-Date -Format 'HH:mm:ss'), $p.Id, (Split-Path $file -Leaf))
            Say ("已写入 minidump: {0} ({1:N0} MB)" -f $file, ((Get-Item $file).Length / 1MB)) Green
        } else { Say "MiniDumpWriteDump 失败 (进程 $($p.Id))" Red; Remove-Item -LiteralPath $file -ErrorAction SilentlyContinue }
    }
    exit 0
}

# ------------------------------------------------------------------ the session
$Start = Get-Date
$Logs = Join-Path $Root 'logs'
$Reports = Join-Path $Root 'reports'
New-Item -ItemType Directory -Force -Path $Logs, $Reports, (Join-Path $Root 'cache') | Out-Null
$SessionName = '{0}_{1}_{2}' -f $Start.ToString('yyyyMMdd-HHmmss'), ($env:COMPUTERNAME -replace '[^A-Za-z0-9_-]', ''), $Mode
$Session = Join-Path $Logs $SessionName
New-Item -ItemType Directory -Force -Path $Session | Out-Null
$Log = Join-Path $Session 'game.log'

Say "ssbu-recomp 测试包  build $BuildId  模式 $Mode" Cyan
Say "本次运行的日志文件夹: $Session"

# the run this replays
$Source = $null; $SourceInfo = $null
if ($Mode -eq 'replay') {
    if (-not $Report) { Say '用法: 把报告 zip (或会话文件夹) 拖到 Replay.bat 上。' Yellow; exit 1 }
    $Source = Join-Path $Session 'source'
    if ((Get-Item -LiteralPath $Report).PSIsContainer) { Copy-Item -LiteralPath $Report -Destination $Source -Recurse }
    else { Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::ExtractToDirectory((Full $Report), $Source) }
    $inner = @(Get-ChildItem -LiteralPath $Source -Directory)
    if (-not (Test-Path -LiteralPath (Join-Path $Source 'session.json')) -and $inner.Count -eq 1) { $Source = $inner[0].FullName }
    $SourceInfo = [IO.File]::ReadAllText((Join-Path $Source 'session.json'), [Text.Encoding]::UTF8) | ConvertFrom-Json
    Say ("原始运行: {0}, 机器 {1}, build {2}, 结果 {3}" -f $SourceInfo.session, $SourceInfo.computer, $SourceInfo.build_id, $SourceInfo.result)
    if ($SourceInfo.build_id -ne $BuildId) {
        Say "报告来自 build $($SourceInfo.build_id)，本包是 build $BuildId。要复现，请用同一个 build 的包 (开发机: build\dist\ 下按 build 号保留)。" Red
        if (-not $Force) { exit 3 }
        Say '-Force: 仍然继续 (结果不能代表原始运行)。' Yellow
    }
    if (-not (Test-Path -LiteralPath (Join-Path $Source 'input.rec'))) { Say '报告里没有 input.rec，只能复现设置和存档，不能复现操作。' Yellow }
}

# paths and settings
$Romfs = Full (Setting 'romfs' 'romfs')
$UserData = Full (Setting 'userdata' 'userdata')
$Seconds = if ($Seconds -ge 0) { $Seconds } else { [int](Setting 'seconds' '0') }
$GameEnv = [ordered]@{}
$Inherited = [ordered]@{}
Get-ChildItem env: | Where-Object { $_.Name -like 'SSBU_*' -or $_.Name -like 'RECOMP_*' } | ForEach-Object { $Inherited[$_.Name] = $_.Value; Remove-Item -LiteralPath "env:$($_.Name)" }

$Scratch = Join-Path (Join-Path $Root 'tmp') $SessionName           # the throwaway save of smoke / replay: not in the report
if ($Mode -eq 'smoke') {
    $UserData = Join-Path $Scratch 'userdata'
    New-Item -ItemType Directory -Force -Path $UserData | Out-Null
    $seed = Join-Path $Root 'tests\smoke_save'
    if (Test-Path -LiteralPath $seed) { Get-ChildItem -LiteralPath $seed -Force | Copy-Item -Destination $UserData -Recurse -Force }
    $GameEnv['SSBU_SHOT_DIR'] = $Session
    if ($Match) {
        # the fast test of tests/DEBUG_MODE.md at the normal speed. The sequence finds the screens by their draw counts: on
        # 2026-10-15 those of the build no longer matched (opening movie 18 draws, the sequence waits for 22-26), so this
        # passes only with sequences made again for the build
        $seq = Join-Path $Root 'tests\seq_battle_play_fast.txt'
        $GameEnv['SSBU_SEQ'] = (@(Get-Content -LiteralPath $seq | Where-Object { $_.Trim() -and -not $_.StartsWith('#') } | ForEach-Object { $_.Trim() }) -join ' ')
        $GameEnv['SSBU_SKIP_MOVIES'] = '1'; $GameEnv['SSBU_AUDIO_MUTE'] = '1'; $GameEnv['SSBU_VK_PREP'] = '0'; $GameEnv['SSBU_DEBUG_END'] = '20s'
        $GameEnv['SSBU_SHOT_BATTLE'] = '600'
        if ($Seconds -le 0) { $Seconds = 300 }
    } else {
        # boot check: no input; the opening movie, the title and the game's own demo run; a picture every 20 s
        $GameEnv['SSBU_SHOT'] = '600'; $GameEnv['SSBU_SHOT_EVERY'] = '1200'
        if ($Seconds -le 0) { $Seconds = [int](Setting 'smoke_seconds' '90') }
    }
}
if ($Mode -eq 'replay') {
    $UserData = Join-Path $Scratch 'userdata'
    New-Item -ItemType Directory -Force -Path $UserData | Out-Null
    $sz = Join-Path $Source 'save_before.zip'
    if (Test-Path -LiteralPath $sz) { Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::ExtractToDirectory($sz, $UserData) }
    elseif ("$($SourceInfo.save_snapshot)" -like 'save_before: no save*') { Say '原始运行从空存档开始, 回放也一样。' }
    else { Say "报告里没有 save_before.zip ($($SourceInfo.save_snapshot)): 从空存档开始, 可能和原始运行不同。" Yellow }
    # the original run's options, without its paths (they belong to the other PC)
    $paths = 'SSBU_ROMFS', 'SSBU_USERDATA', 'SSBU_DLC_LIST', 'SSBU_FONTS', 'SSBU_REPORT_DIR', 'SSBU_TRACE', 'SSBU_REC', 'SSBU_FIGHT_HASH', 'SSBU_PLAY', 'SSBU_CONFIG', 'SSBU_GRAPHICS_CONFIG'
    foreach ($p in $SourceInfo.env.PSObject.Properties) { if ($paths -notcontains $p.Name) { $GameEnv[$p.Name] = [string]$p.Value } }
    if (Test-Path -LiteralPath (Join-Path $Source 'input.rec')) { $GameEnv['SSBU_PLAY'] = Join-Path $Source 'input.rec' }
    if (-not $LocalConfig -and (Test-Path -LiteralPath (Join-Path $Source 'config_before\controllers.ini'))) {
        $cfg = Join-Path $Session 'config_replay'
        Copy-Item -LiteralPath (Join-Path $Source 'config_before') -Destination $cfg -Recurse
        $GameEnv['SSBU_CONFIG'] = Join-Path $cfg 'controllers.ini'
        if (Test-Path -LiteralPath (Join-Path $cfg 'graphics.ini')) { $GameEnv['SSBU_GRAPHICS_CONFIG'] = Join-Path $cfg 'graphics.ini' }
    }
} else {
    $fs = Setting 'fixed_seed' 'auto'
    if ($fs -eq 'auto') { $GameEnv['SSBU_FIXED_SEED'] = [string](Get-Random -Minimum 1 -Maximum 2147483647) }
    elseif ($fs -ne 'none' -and $fs -ne '0') { $GameEnv['SSBU_FIXED_SEED'] = $fs }
    if ((Setting 'deterministic' '1') -eq '1') { $GameEnv['SSBU_DETERMINISTIC'] = '1' }
}
$GameEnv['SSBU_ROMFS'] = $Romfs
$GameEnv['SSBU_USERDATA'] = $UserData
$GameEnv['SSBU_FONTS'] = Join-Path $Root 'sysdata\fonts'
if (Test-Path -LiteralPath (Join-Path $Root 'sysdata\zoneinfo')) { $GameEnv['SSBU_TZDIR'] = Join-Path $Root 'sysdata\zoneinfo' }
$GameEnv['SSBU_REPORT_DIR'] = $Session
$GameEnv['SSBU_TRACE'] = Join-Path $Session 'boot_trace.txt'
$GameEnv['SSBU_REC'] = Join-Path $Session 'input.rec'
$GameEnv['SSBU_MINIDUMP'] = Setting 'minidump' '1'
if ((Setting 'fight_hash' '1') -eq '1') { $GameEnv['SSBU_FIGHT_HASH'] = Join-Path $Session 'fight_hash.txt' }
foreach ($k in $Ini.env.Keys) { if ($Mode -ne 'replay' -or -not $GameEnv.Contains($k)) { $GameEnv[$k] = $Ini.env[$k] } }
# Use the same explicit identity as Console; replay keeps the recorded identity when present.
$PlayerName = if ($Mode -eq 'replay' -and $GameEnv.Contains('SSBU_NICKNAME')) { [string]$GameEnv['SSBU_NICKNAME'] }
              elseif ($Ini.run.Contains('game_name')) { [string]$Ini.run['game_name'] }
              elseif ($Ini.Contains('net') -and $Ini.net.Contains('game_name')) { [string]$Ini.net['game_name'] }
              else { '' }
$PlayerName = $PlayerName.Trim()
if (-not $PlayerName -or [Text.Encoding]::UTF8.GetByteCount($PlayerName) -gt 32 -or $PlayerName -match '[\x00-\x1f\x7f-\x9f]') {
    Say '请先打开 SSBU Console，在开始页填写有效的游戏内名字（最多 32 字节）。' Red
    exit 2
}
$GameEnv['SSBU_NICKNAME'] = $PlayerName
New-Item -ItemType Directory -Force -Path $UserData | Out-Null

# before the run: this PC, the package, the state the run starts from
Say '收集系统信息...'
WriteText (Join-Path $Session 'system.txt') (Get-SystemText)
$pre = Test-Preflight $Romfs
WriteText (Join-Path $Session 'preflight.txt') (($pre.lines + ($pre.fatal | ForEach-Object { "FATAL: $_" })) -join "`r`n")
foreach ($l in $pre.lines) { if ($l -like 'WARNING*' -or ($pre.package_bad -and $l -like 'package:*')) { Say $l Yellow } }
$cfgDir = if ($GameEnv.Contains('SSBU_CONFIG')) { Split-Path $GameEnv['SSBU_CONFIG'] } else { Join-Path $Root 'config' }
if (Test-Path -LiteralPath $cfgDir) { Copy-Item -LiteralPath $cfgDir -Destination (Join-Path $Session 'config_before') -Recurse }
$snap = if ($Mode -eq 'replay') { 'save_before: the replayed run''s save (source\save_before.zip)' } else { Save-Snapshot $UserData (Join-Path $Session 'save_before.zip') ([int](Setting 'save_snapshot_max_mb' '200')) }

$Affinity = $null
$ex = Setting 'exclude_cpus' ''
if ($ex) {
    $me = [Diagnostics.Process]::GetCurrentProcess()
    $mask = [int64]$me.ProcessorAffinity
    foreach ($c in $ex.Split(',')) { if ($c.Trim()) { $mask = $mask -band (-bnot ([int64]1 -shl [int]$c.Trim())) } }
    if ($mask -ne 0) { $me.ProcessorAffinity = [IntPtr]$mask; $Affinity = '{0:x}' -f $mask }       # the game inherits it
}

$Info = [ordered]@{
    session = $SessionName; mode = $Mode; build_id = $BuildId; computer = $env:COMPUTERNAME; user = $env:USERNAME
    started = $Start.ToString('o'); package = $Root; seconds_limit = $Seconds; affinity_mask = $Affinity
    git_commit = $(if ($Manifest) { $Manifest.git_commit } else { $null }); package_problems = $pre.package_bad
    replay_of = $(if ($SourceInfo) { $SourceInfo.session } else { $null }); save_snapshot = $snap
    env = $GameEnv; inherited_env_removed = $Inherited
}
if ($pre.fatal.Count) {
    foreach ($f in $pre.fatal) { Say "无法启动: $f" Red }
    $Info['result'] = 'not_started'; $Info['problems'] = $pre.fatal
    WriteText (Join-Path $Session 'session.json') ($Info | ConvertTo-Json -Depth 5)
    if (-not $NoPause) { Read-Host '按回车退出' | Out-Null }
    exit 2
}

# the run: stdout and stderr of the game in one file, in order
foreach ($k in $GameEnv.Keys) { Set-Item -LiteralPath "env:$k" -Value $GameEnv[$k] }
WriteText (Join-Path $Session 'session.json') ($Info | ConvertTo-Json -Depth 5)
$psi = New-Object Diagnostics.ProcessStartInfo
$psi.FileName = Join-Path $env:SystemRoot 'System32\cmd.exe'
$psi.Arguments = '/d /s /c ""{0}" "..\binaries\13.0.4\main.elf" {1} > "{2}" 2>&1"' -f $Exe, $Seconds, $Log
$psi.WorkingDirectory = $GameDir
$psi.UseShellExecute = $false
$t0 = Get-Date
$proc = [Diagnostics.Process]::Start($psi)
Start-Sleep -Milliseconds 800
$game = Get-CimInstance Win32_Process -Filter "ParentProcessId=$($proc.Id)" -ErrorAction SilentlyContinue | Where-Object { $_.Name -eq 'ssbu_native.exe' } | Select-Object -First 1
WriteText (Join-Path $Session 'running.txt') ("{0}`r`n" -f $(if ($game) { $game.ProcessId } else { $proc.Id }))
Say $(switch ($Mode) { 'smoke' { if ($Match) { "冒烟测试 (自动对战) 运行中, 最多 $Seconds 秒..." } else { "冒烟测试运行中: 游戏自己运行 $Seconds 秒, 请不要操作..." } } 'replay' { '回放运行中: 手柄/键盘输入被录像代替，关闭游戏窗口结束。' } default { '游戏运行中。关闭游戏窗口后会自动整理日志。如果游戏卡住不动，可运行 Dump.bat 再关闭游戏。' } }) Green
$proc.WaitForExit()
$Code = $proc.ExitCode
$End = Get-Date
Remove-Item -LiteralPath (Join-Path $Session 'running.txt') -ErrorAction SilentlyContinue

# after the run
$v = Get-Verdict $Code $Log $Session
if ($Mode -eq 'smoke') {
    $clean = $v.result -eq 'ok'                                      # no crash, trap, device loss, freeze report; exit code 0
    $v.result = if ($Match) { if ($clean -and $v.markers.Contains('debug_end')) { 'pass' } else { 'fail' } }
                else { if ($clean -and $v.markers.Contains('time_limit') -and $v.markers.Contains('movie')) { 'pass' } else { 'fail' } }
}
$meaning = if ($ExitMeaning.ContainsKey($Code)) { $ExitMeaning[$Code] } else { 'exit code 0x{0:X8}' -f $Code }
WriteText (Join-Path $Session 'events.txt') (Get-Events $t0.AddSeconds(-5))
$cfgAfter = Join-Path $Root 'config'
if ($Mode -ne 'replay' -and (Test-Path -LiteralPath $cfgAfter)) { Copy-Item -LiteralPath $cfgAfter -Destination (Join-Path $Session 'config_after') -Recurse }
foreach ($f in 'fighter_report.txt') { $p = Join-Path $GameDir $f; if ((Test-Path -LiteralPath $p) -and (Get-Item -LiteralPath $p).LastWriteTime -ge $t0) { Copy-Item -LiteralPath $p -Destination $Session } }
$compare = $null
if ($Mode -eq 'replay') { $compare = Compare-FightHash (Join-Path $Source 'fight_hash.txt') (Join-Path $Session 'fight_hash.txt') }

$Info['ended'] = $End.ToString('o'); $Info['duration_s'] = [math]::Round(($End - $t0).TotalSeconds, 1)
$Info['exit_code'] = $Code; $Info['exit_meaning'] = $meaning; $Info['result'] = $v.result
$Info['log_markers'] = $v.markers; $Info['dumps'] = $v.dumps
if ($compare) { $Info['replay_compare'] = $compare }
WriteText (Join-Path $Session 'session.json') ($Info | ConvertTo-Json -Depth 5)

$color = @{ ok = 'Green'; pass = 'Green'; warning = 'Yellow'; error = 'Red'; fail = 'Red' }[$v.result]
Say ''
Say ("结果: {0}   退出码 {1} ({2})   用时 {3:N0} 秒" -f $v.result.ToUpper(), $Code, $meaning, ($End - $t0).TotalSeconds) $color
foreach ($k in $v.markers.Keys) { Say "  $k : $($v.markers[$k])" }
if ($v.dumps.Count) { Say "  minidump: $($v.dumps -join ', ')" }
if ($compare) { Say $compare Cyan }

$bad = $v.result -in 'error', 'fail', 'warning'
if ($bad -and $Mode -eq 'play' -and -not $NoPause) {
    $note = Read-Host '请简单描述出错前在做什么 (可留空，回车跳过)'
    if ($note) { AddText (Join-Path $Session 'note.txt') ("{0}: {1}" -f (Get-Date -Format 'HH:mm:ss'), $note) }
}
$zipWhen = Setting 'zip' 'always'
if ($zipWhen -eq 'always' -or ($zipWhen -eq 'error' -and $bad)) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $zip = Join-Path $Reports "$SessionName.zip"
    try {
        [IO.Compression.ZipFile]::CreateFromDirectory($Session, $zip, [IO.Compression.CompressionLevel]::Optimal, $true)
        Say ("报告: {0} ({1:N1} MB)" -f $zip, ((Get-Item $zip).Length / 1MB))
        if ($bad -and $Mode -ne 'replay') {
            Say '请把这个 zip 发给开发者，用 Replay.bat 和 minidump 在开发机上复现。' Yellow
            if (-not $NoPause) { Start-Process explorer.exe -ArgumentList "/select,`"$zip`"" }
        }
    } catch { Say "报告 zip 未能生成: $($_.Exception.Message); 日志在 $Session" Red }
}
if (Test-Path -LiteralPath $Scratch) { Remove-Item -LiteralPath $Scratch -Recurse -Force -ErrorAction SilentlyContinue }
Trim-Sessions $Logs ([int](Setting 'keep_sessions' '30'))
if (($bad -or $Mode -ne 'play') -and -not $NoPause) { Read-Host '按回车关闭' | Out-Null }
exit $(if ($v.result -in 'ok', 'pass', 'warning') { 0 } else { 1 })
