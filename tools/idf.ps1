<#
.SYNOPSIS
    Run `idf.py` with a fully exported ESP-IDF environment.

.DESCRIPTION
    VS Code tasks in this project call this script instead of `idf.py`
    directly. That keeps the tasks working from any terminal — plain
    PowerShell, the VS Code integrated terminal, or a build agent — without
    requiring the ESP-IDF VS Code extension to have pre-exported the
    environment, and without hardcoding a machine-specific IDF path into
    .vscode/tasks.json.

    See tools/_idf_env.ps1 for how ESP-IDF is located. Put a machine-specific
    path in .idf-env.json at the repository root (git-ignored):

        { "idfPath": "C:\\esp\\v5.4\\esp-idf",
          "toolsPath": "C:\\esp\\tools" }

.EXAMPLE
    pwsh -File tools/idf.ps1 build
    pwsh -File tools/idf.ps1 -p COM7 flash monitor
#>
#Requires -Version 5.1

# NO param block and NO [CmdletBinding()] here, deliberately.
#
# [CmdletBinding()] makes PowerShell add its own common parameters, including
# -ProgressAction and -PipelineVariable. A caller then running the single most
# common idf.py invocation fails before the script starts:
#
#   tools/idf.ps1 -p COM4 flash
#   -> The parameter name "p" is ambiguous. Possible matches include:
#      -ProgressAction -PipelineVariable.
#
# Without a param block, PowerShell passes every argument through untouched and
# they land in $args, so `-p COM4 flash` reaches idf.py exactly as typed.
$IdfArgs = $args

$ErrorActionPreference = 'Stop'

. (Join-Path $PSScriptRoot '_idf_env.ps1')

$repoRoot = Get-RepoRoot
$idfPath  = Initialize-IdfEnvironment -RepoRoot $repoRoot
if (-not $idfPath) { exit 3 }

if (-not (Get-Command idf.py -ErrorAction SilentlyContinue)) {
    Write-Host 'ERROR: export.ps1 completed but idf.py is still not on PATH.' -ForegroundColor Red
    Write-Host "Run the installer for this target first:"
    Write-Host "  & '$idfPath\install.ps1' esp32s3"
    exit 4
}

if (-not $IdfArgs -or $IdfArgs.Count -eq 0) {
    Write-Host 'No arguments given. Common commands:'
    Write-Host '  build                    build the application'
    Write-Host '  -p COM7 flash            flash over the given port'
    Write-Host '  monitor                  open the serial monitor'
    Write-Host '  build flash monitor      the everyday loop'
    exit 0
}

Push-Location $repoRoot
try {
    & idf.py @IdfArgs
    exit $LASTEXITCODE
}
finally {
    Pop-Location
}
