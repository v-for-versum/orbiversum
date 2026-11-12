<#
.SYNOPSIS
    Pack ./assets into a SPIFFS image and flash it into the `storage` partition.

.DESCRIPTION
    The application reads runtime assets (images, sounds, certificates) from a
    SPIFFS partition mounted at /assets. Updating them does not require
    reflashing the application, which matters during UI work: an asset swap is
    a two-second operation instead of a full build-and-flash cycle.

    The partition offset and size are resolved from partitions.csv by
    ESP-IDF's own gen_esp32part.py, so blank (auto-assigned) offsets stay
    correct even if the table is edited.

.PARAMETER Port
    Serial port to flash over.

.PARAMETER AssetsDir
    Source folder. Defaults to <repo>/assets.

.PARAMETER OnlyBuild
    Build the image but do not flash it.

.EXAMPLE
    pwsh -File tools/upload_fs.ps1 -Port COM7
    pwsh -File tools/upload_fs.ps1 -Port COM7 -AssetsDir .\assets
#>
#Requires -Version 5.1
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $Port,

    [string] $AssetsDir,

    [switch] $OnlyBuild
)

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot '_idf_env.ps1')

function Write-Step { param([string] $m) Write-Host "==> $m" -ForegroundColor Cyan }
function Write-Fail { param([string] $m) Write-Host "ERROR: $m" -ForegroundColor Red }

$repoRoot = Get-RepoRoot

if (-not $AssetsDir) { $AssetsDir = Join-Path $repoRoot 'assets' }

if (-not (Test-Path -LiteralPath $AssetsDir -PathType Container)) {
    Write-Fail "Assets folder not found: $AssetsDir"
    Write-Host 'Create it and put the files you want on the device inside.'
    exit 2
}
$assetsFull = (Resolve-Path -LiteralPath $AssetsDir).Path

$idfPath = Initialize-IdfEnvironment -RepoRoot $repoRoot
if (-not $idfPath) { exit 3 }

# --- Resolve the storage partition ------------------------------------------
$partitionCsv = Join-Path $repoRoot 'partitions.csv'
if (-not (Test-Path -LiteralPath $partitionCsv)) {
    Write-Fail "partitions.csv not found at $partitionCsv"
    exit 4
}

$genPart = Join-Path $idfPath 'components/partition_table/gen_esp32part.py'
if (-not (Test-Path -LiteralPath $genPart)) {
    Write-Fail "gen_esp32part.py not found at $genPart"
    exit 4
}

$resolvedCsv = Join-Path ([System.IO.Path]::GetTempPath()) 'guition_knob_partitions.csv'
Write-Step 'Resolving partition offsets'
& python $genPart $partitionCsv $resolvedCsv
if ($LASTEXITCODE -ne 0) {
    Write-Fail "gen_esp32part.py failed with exit code $LASTEXITCODE"
    exit 5
}

function ConvertTo-Bytes {
    param([string] $Value)
    $v = $Value.Trim().ToUpperInvariant()
    if     ($v.EndsWith('K')) { return [int64]([double]$v.TrimEnd('K') * 1KB) }
    elseif ($v.EndsWith('M')) { return [int64]([double]$v.TrimEnd('M') * 1MB) }
    else                      { return [int64]$v }
}

$storage = $null
foreach ($line in Get-Content -LiteralPath $resolvedCsv) {
    $trimmed = $line.Trim()
    if (-not $trimmed -or $trimmed.StartsWith('#')) { continue }
    $fields = $trimmed.Split(',')
    if ($fields.Count -lt 5) { continue }
    if ($fields[0].Trim() -eq 'storage') {
        $storage = [pscustomobject]@{
            Offset = [Convert]::ToInt64($fields[3].Trim(), 16)
            Size   = ConvertTo-Bytes $fields[4]
        }
        break
    }
}

if (-not $storage) {
    Write-Fail "No partition named 'storage' in $partitionCsv"
    exit 6
}

Write-Step ("storage partition: offset 0x{0:X}, size {1:N0} bytes" -f $storage.Offset, $storage.Size)

# --- Build the SPIFFS image -------------------------------------------------
$spiffsgen = Join-Path $idfPath 'components/spiffs/spiffsgen.py'
if (-not (Test-Path -LiteralPath $spiffsgen)) {
    Write-Fail "spiffsgen.py not found at $spiffsgen"
    exit 7
}

$outDir = Join-Path $repoRoot 'build'
if (-not (Test-Path -LiteralPath $outDir)) {
    New-Item -ItemType Directory -Path $outDir -Force | Out-Null
}
$imagePath = Join-Path $outDir 'storage.bin'

Write-Step "Packing $assetsFull"
& python $spiffsgen $storage.Size $assetsFull $imagePath --page-size 256 --obj-name-len 32 --meta-len 4 --use-magic
if ($LASTEXITCODE -ne 0) {
    Write-Fail "spiffsgen.py failed with exit code $LASTEXITCODE"
    Write-Host 'If the image does not fit, move assets to the SD card or enlarge the storage partition.'
    exit 8
}

$imageSize = (Get-Item -LiteralPath $imagePath).Length
Write-Step ("Image: {0} ({1:N0} bytes, {2:P1} of the partition)" -f $imagePath, $imageSize, ($imageSize / $storage.Size))

if ($OnlyBuild) {
    Write-Host ''
    Write-Host 'Built only (-OnlyBuild). Not flashing.' -ForegroundColor Yellow
    exit 0
}

# --- Flash ------------------------------------------------------------------
Write-Step "Flashing to $Port"
& esptool.py --chip esp32s3 --port $Port --baud 921600 `
    write-flash ("0x{0:X}" -f $storage.Offset) $imagePath
if ($LASTEXITCODE -ne 0) {
    Write-Fail "esptool.py failed with exit code $LASTEXITCODE"
    exit 9
}

Write-Host ''
Write-Host 'Done. The files are now on the device at /assets.' -ForegroundColor Green
