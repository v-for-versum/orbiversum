<#
.SYNOPSIS
    Shared ESP-IDF environment bootstrap for the scripts in tools/.

.DESCRIPTION
    Dot-source this file, then call Initialize-IdfEnvironment. It locates
    ESP-IDF, exports it, and returns the resolved path. Kept separate from
    idf.ps1 so that upload_fs.ps1 can reuse exactly the same discovery rules
    instead of duplicating them.

    Discovery order:
      1. $env:IDF_PATH, if it is already exported.
      2. .idf-env.json in the repository root (git-ignored, per developer).
      3. The default locations used by the official installers.
#>

function Get-RepoRoot {
    return (Split-Path -Parent $PSScriptRoot)
}

function Find-IdfPath {
    <#
        Returns the resolved ESP-IDF directory, or $null. Never writes to the
        environment — Initialize-IdfEnvironment does that.
    #>
    [CmdletBinding()]
    param([string] $RepoRoot = (Get-RepoRoot))

    $idfPath   = $env:IDF_PATH
    $toolsPath = $env:IDF_TOOLS_PATH

    $configFile = Join-Path $RepoRoot '.idf-env.json'
    if (Test-Path -LiteralPath $configFile) {
        try {
            $config = Get-Content -LiteralPath $configFile -Raw | ConvertFrom-Json
            if (-not $idfPath   -and $config.idfPath)   { $idfPath   = $config.idfPath }
            if (-not $toolsPath -and $config.toolsPath) { $toolsPath = $config.toolsPath }
        }
        catch {
            Write-Host "ERROR: could not parse .idf-env.json: $($_.Exception.Message)" -ForegroundColor Red
            return $null
        }
    }

    if ($idfPath -and (Test-Path -LiteralPath (Join-Path $idfPath 'export.ps1'))) {
        return (Resolve-Path -LiteralPath $idfPath).Path
    }

    $candidates = @(
        (Join-Path $HOME 'esp/esp-idf')
        (Join-Path $HOME 'esp/v5.5/esp-idf')
        (Join-Path $HOME 'esp/v5.4/esp-idf')
        (Join-Path $HOME 'esp/v5.3/esp-idf')
        (Join-Path $HOME 'esp/v5.2/esp-idf')
        'C:/Espressif/frameworks/esp-idf-v5.5'
        'C:/Espressif/frameworks/esp-idf-v5.4'
        'C:/Espressif/frameworks/esp-idf-v5.3'
    )
    foreach ($candidate in $candidates) {
        if (Test-Path -LiteralPath (Join-Path $candidate 'export.ps1')) {
            return (Resolve-Path -LiteralPath $candidate).Path
        }
    }

    # Last resort: the versioned ~/esp/v5.x/esp-idf layout, newest first.
    $globbed = Get-ChildItem -Path (Join-Path $HOME 'esp') -Directory -Filter 'v5.*' -ErrorAction SilentlyContinue |
               Sort-Object Name -Descending |
               ForEach-Object { Join-Path $_.FullName 'esp-idf' } |
               Where-Object { Test-Path -LiteralPath (Join-Path $_ 'export.ps1') }
    if ($globbed) { return (Resolve-Path -LiteralPath $globbed[0]).Path }

    return $null
}

function Write-IdfNotFoundHelp {
    [CmdletBinding()]
    param([string] $IdfPath)
    Write-Host 'ERROR: ESP-IDF was not found.' -ForegroundColor Red
    Write-Host ''
    Write-Host 'Install it with the official installer, or point this project at an existing copy:'
    Write-Host ''
    Write-Host '  # 1. install (once)'
    Write-Host '  mkdir $HOME\esp; cd $HOME\esp'
    Write-Host '  git clone -b v5.4 --recursive https://github.com/espressif/esp-idf.git'
    Write-Host '  cd esp-idf; .\install.ps1 esp32s3'
    Write-Host ''
    Write-Host '  # 2. tell this project where it is'
    Write-Host '  ''{ "idfPath": "C:\\esp\\v5.4\\esp-idf" }'' | Set-Content .idf-env.json'
    Write-Host ''
}

function Initialize-IdfEnvironment {
    <#
        Locates and exports ESP-IDF into the current process. Returns the
        resolved IDF path, or $null on failure (after printing guidance).
    #>
    [CmdletBinding()]
    param([string] $RepoRoot = (Get-RepoRoot))

    $idfPath = Find-IdfPath -RepoRoot $RepoRoot
    if (-not $idfPath) {
        Write-IdfNotFoundHelp
        return $null
    }

    # export.ps1 must be dot-sourced so its assignments land in this session.
    # Environment changes are process-wide, so doing it inside a function is
    # safe.
    . (Join-Path $idfPath 'export.ps1') | Out-Null

    $env:IDF_PATH = $idfPath
    Write-Host "==> ESP-IDF: $idfPath" -ForegroundColor Cyan
    return $idfPath
}
