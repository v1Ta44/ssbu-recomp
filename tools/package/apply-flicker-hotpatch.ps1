param(
    [ValidateSet('Apply', 'Rollback')][string]$Action = 'Apply',
    [string]$Root = (Split-Path -Parent $PSScriptRoot)
)
$ErrorActionPreference = 'Stop'
$Patch = Get-Content -LiteralPath (Join-Path $PSScriptRoot 'patch.json') -Raw -Encoding UTF8 | ConvertFrom-Json
$Root = (Resolve-Path -LiteralPath $Root).Path
$ManifestPath = Join-Path $Root 'manifest.json'
$Backup = Join-Path $Root ('.hotpatch-backup\' + $Patch.id)
function Digest([string]$Path) {
    $stream = [IO.File]::OpenRead($Path)
    $hasher = [Security.Cryptography.SHA256]::Create()
    try { ([BitConverter]::ToString($hasher.ComputeHash($stream))).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $hasher.Dispose() }
}
function Assert-Hash([string]$Path, [string]$Hash) {
    if (-not (Test-Path -LiteralPath $Path -PathType Leaf) -or (Digest $Path) -ne $Hash) { throw "File does not match this patch: $Path" }
}
function Destination([string]$Relative) {
    $p = [IO.Path]::GetFullPath((Join-Path $Root $Relative))
    if (-not $p.StartsWith($Root.TrimEnd('\') + '\', [StringComparison]::OrdinalIgnoreCase)) { throw "Invalid patch path: $Relative" }
    $p
}
# Rename complete temporary files rather than overwrite hard-linked distribution files.
function Replace-File([string]$Source, [string]$Destination) {
    $temp = $Destination + '.hotpatch-' + [Guid]::NewGuid().ToString('N')
    try { Copy-Item -LiteralPath $Source -Destination $temp; Move-Item -LiteralPath $temp -Destination $Destination -Force }
    finally { if (Test-Path -LiteralPath $temp) { Remove-Item -LiteralPath $temp } }
}
function Assert-Unlocked([string]$Path) {
    $stream = [IO.File]::Open($Path, [IO.FileMode]::Open, [IO.FileAccess]::ReadWrite, [IO.FileShare]::None)
    $stream.Dispose()
}
$Manifest = Get-Content -LiteralPath $ManifestPath -Raw -Encoding UTF8 | ConvertFrom-Json
$Records = @($Patch.files.PSObject.Properties)
$Paths = @($Records.Name) + @('manifest.json')
foreach ($rel in $Paths) { Assert-Unlocked (Destination $rel) }
if ($Action -eq 'Rollback') {
    $statePath = Join-Path $Backup 'installed.json'
    $state = Get-Content -LiteralPath $statePath -Raw -Encoding UTF8 | ConvertFrom-Json
    if ($Manifest.hotpatch.id -ne $Patch.id) { throw 'This patch is not currently installed.' }
    foreach ($p in $state.after.PSObject.Properties) { Assert-Hash (Destination $p.Name) $p.Value }
    foreach ($p in $state.before.PSObject.Properties) { Assert-Hash (Join-Path $Backup $p.Name) $p.Value }
    foreach ($rel in $Paths) { Replace-File (Join-Path $Backup $rel) (Destination $rel) }
    Move-Item -LiteralPath $statePath -Destination (Join-Path $Backup ('rolled-back-' + [Guid]::NewGuid().ToString('N') + '.json'))
    Write-Host 'Rollback complete. The original executable and package manifest have been restored.' -ForegroundColor Green
    exit 0
}
if ($Manifest.hotpatch.id -eq $Patch.id) {
    $state = Get-Content -LiteralPath (Join-Path $Backup 'installed.json') -Raw -Encoding UTF8 | ConvertFrom-Json
    foreach ($p in $state.after.PSObject.Properties) { Assert-Hash (Destination $p.Name) $p.Value }
    Write-Host 'This patch is already installed and verified.' -ForegroundColor Green
    exit 0
}
if ($Manifest.build_id -ne $Patch.target_build_id) { throw 'This patch supports only yuzu-all-features-20261018-98528e1 (build 6525ebfdd221).' }
foreach ($record in $Records) {
    Assert-Hash (Destination $record.Name) $record.Value.old_sha256
    Assert-Hash (Join-Path $PSScriptRoot ('payload/' + $record.Name)) $record.Value.sha256
}
foreach ($p in $Patch.dependencies.PSObject.Properties) { Assert-Hash (Destination $p.Name) $p.Value }
if (Test-Path -LiteralPath $Backup) {
    Move-Item -LiteralPath $Backup -Destination ($Backup + '.previous-' + [Guid]::NewGuid().ToString('N'))
}
New-Item -ItemType Directory -Path $Backup -Force | Out-Null
$before = [ordered]@{}
foreach ($rel in $Paths) {
    $src = Destination $rel; $dst = Join-Path $Backup $rel
    New-Item -ItemType Directory -Path (Split-Path -Parent $dst) -Force | Out-Null
    Copy-Item -LiteralPath $src -Destination $dst
    $before[$rel] = Digest $src
    Assert-Hash $dst $before[$rel]
}
try {
    foreach ($record in $Records) { Replace-File (Join-Path $PSScriptRoot ('payload/' + $record.Name)) (Destination $record.Name) }
    $Manifest.build_id = $Patch.target_build_id + '-fp6-bg'
    $Manifest | Add-Member -NotePropertyName hotpatch -NotePropertyValue ([ordered]@{
        id = $Patch.id; applied = [DateTime]::Now.ToString('s'); original_build_id = $Patch.target_build_id
        source_commit = $Patch.source_commit; upstream_pr = $Patch.upstream_pr; focus_pause_default = $false
    }) -Force
    foreach ($record in $Records) {
        $Manifest.files.($record.Name).size = $record.Value.size
        $Manifest.files.($record.Name).sha256 = $record.Value.sha256
    }
    $Manifest.pe.'game/ssbu_native.exe' = $Patch.pe
    $newManifest = Join-Path $Backup 'new-manifest.json'
    $Manifest | ConvertTo-Json -Depth 50 | Set-Content -LiteralPath $newManifest -Encoding UTF8
    Replace-File $newManifest $ManifestPath
    foreach ($record in $Records) { Assert-Hash (Destination $record.Name) $record.Value.sha256 }
    $after = [ordered]@{}
    foreach ($rel in $Paths) { $after[$rel] = Digest (Destination $rel) }
    [ordered]@{before=$before;after=$after} | ConvertTo-Json -Depth 10 | Set-Content -LiteralPath (Join-Path $Backup 'installed.json') -Encoding UTF8
} catch {
    $failure = $_
    foreach ($rel in $Paths) { Replace-File (Join-Path $Backup $rel) (Destination $rel) }
    throw $failure
}
Write-Host 'Patch installed. Floor flicker fix enabled; the game keeps running when unfocused.' -ForegroundColor Green
Write-Host 'The first launch can take longer while Vulkan shaders are prepared. Start the game with the usual launcher.'
