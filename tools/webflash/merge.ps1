<#
.SYNOPSIS
    Собирает единый образ прошивки OBOD для записи из браузера.

.DESCRIPTION
    Берёт файлы и адреса из build/flash_args — то есть ровно то, что `idf.py flash`
    передаёт esptool, — и склеивает их в один образ, который можно записать с
    нулевого адреса. Такой образ понимает esp-web-tools (страница index.html в
    этом каталоге). Сам esptool ничего в образе не патчит: режим, частоту и размер
    flash задаём теми же значениями, что в flash_args, иначе прибор после записи
    из браузера не запустится.

    Запускать после каждой сборки прошивки, повторно — сколько нужно:

        pwsh -File tools/webflash/merge.ps1

    Результат по умолчанию: tools/webflash/firmware/OBOD.bin
    Заодно обновляются поля манифеста — version, а также image.size и
    image.sha256 собранного образа (version по умолчанию — дата и время
    сборки приложения; ключ -Version задаёт номер выпуска), чтобы страница
    показывала версию, размер и хеш того, что лежит рядом. Размер берётся из
    файла, а не из заголовка ответа сервера: на GitHub Pages ответ сжатый, и
    Content-Length описывает не тот файл, который скачивается.

    И пересобирается changelog.json — история всех версий из CHANGELOG.md, —
    чтобы страница записи показывала под кнопкой то, что предлагает записать,
    а страница истории все выпуски. Собирает его tools/make-changelog-json.ps1:
    разбор CHANGELOG.md живёт там, чтобы у данных был один хозяин. Разметка в
    файл не переносится: страница знает только номера версий, подзаголовки
    разделов и пункты списка.

    Скрипт ничего не собирает: ни idf.py build, ни ninja не вызываются.
    Если build/flash_args устарел, сначала соберите прошивку сами.

.PARAMETER BuildDir
    Каталог сборки ESP-IDF. По умолчанию build/ в корне репозитория.

.PARAMETER Output
    Куда положить объединённый образ.
    По умолчанию tools/webflash/firmware/OBOD.bin.

.PARAMETER Chip
    Целевой чип для esptool. По умолчанию esp32s3.

.PARAMETER Version
    Что записать в поле version манифеста. По умолчанию — дата и время
    сборки приложения. Для выпуска укажите номер версии, например 1.0.0.

.PARAMETER SkipManifest
    Не трогать version в manifest.json и не пересобирать changelog.json.

.EXAMPLE
    pwsh -File tools/webflash/merge.ps1
    pwsh -File tools/webflash/merge.ps1 -BuildDir build -Output C:\temp\OBOD.bin
#>
#Requires -Version 5.1

[CmdletBinding()]
param(
    [string] $BuildDir,
    [string] $Output,
    [string] $Chip = 'esp32s3',
    [string] $Version,
    [switch] $SkipManifest
)

$ErrorActionPreference = 'Stop'

$scriptDir = $PSScriptRoot
if (-not $scriptDir) { $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path }
$toolsDir  = Split-Path -Parent $scriptDir

# Корень репозитория: через общий помощник из tools/_idf_env.ps1, он же ищет IDF.
$envHelper = Join-Path $toolsDir '_idf_env.ps1'
if (Test-Path -LiteralPath $envHelper) { . $envHelper }
if (Get-Command Get-RepoRoot -ErrorAction SilentlyContinue) {
    $repoRoot = Get-RepoRoot
} else {
    $repoRoot = Split-Path -Parent $toolsDir
}

if (-not $BuildDir) { $BuildDir = Join-Path $repoRoot 'build' }
if (-not $Output)   { $Output   = Join-Path $scriptDir 'firmware\OBOD.bin' }

function Write-Step  { param([string] $Text) Write-Host $Text -ForegroundColor Cyan }
function Write-Warn  { param([string] $Text) Write-Host $Text -ForegroundColor Yellow }

# ---------------------------------------------------------------------------
# 1. Части образа: адреса и имена — только из сборки.
# ---------------------------------------------------------------------------

function Read-FlashParts {
    <#
        Разбирает build/flash_args. Если его нет, читает build/flasher_args.json.
        Возвращает @{ FlashMode; FlashFreq; FlashSize; Parts = @( @{ Offset; File; Size } ) }
        с адресами в виде целых чисел и путями, уже разрешёнными относительно BuildDir.
    #>
    [CmdletBinding()]
    param([string] $BuildDir)

    $argsFile = Join-Path $BuildDir 'flash_args'
    $jsonFile = Join-Path $BuildDir 'flasher_args.json'

    $flashMode = $null
    $flashFreq = $null
    $flashSize = $null
    $parts     = @()

    if (Test-Path -LiteralPath $argsFile) {
        $lines = Get-Content -LiteralPath $argsFile |
                 Where-Object { $_.Trim() -and -not $_.TrimStart().StartsWith('#') }

        foreach ($line in $lines) {
            $tokens = @($line -split '\s+' | Where-Object { $_ })
            for ($i = 0; $i -lt $tokens.Count; $i++) {
                $token = $tokens[$i]
                if ($token -match '^--flash-(mode|freq|size)$') {
                    $value = if ($i + 1 -lt $tokens.Count) { $tokens[$i + 1] } else { $null }
                    switch ($Matches[1]) {
                        'mode' { $flashMode = $value }
                        'freq' { $flashFreq = $value }
                        'size' { $flashSize = $value }
                    }
                    $i++
                    continue
                }
                if ($token -match '^0x[0-9a-fA-F]+$') {
                    $file = if ($i + 1 -lt $tokens.Count) { $tokens[$i + 1] } else { $null }
                    if (-not $file) { throw "В $argsFile у адреса $token нет имени файла." }
                    $parts += [pscustomobject]@{
                        Offset = [Convert]::ToInt64($token, 16)
                        OffsetText = $token
                        Relative = $file
                        Path = (Join-Path $BuildDir $file)
                    }
                    $i++
                }
            }
        }
    }
    elseif (Test-Path -LiteralPath $jsonFile) {
        $json = Get-Content -LiteralPath $jsonFile -Raw | ConvertFrom-Json
        if ($json.flash_settings) {
            $flashMode = $json.flash_settings.flash_mode
            $flashFreq = $json.flash_settings.flash_freq
            $flashSize = $json.flash_settings.flash_size
        }
        foreach ($property in $json.flash_files.PSObject.Properties) {
            $parts += [pscustomobject]@{
                Offset = [Convert]::ToInt64($property.Name, 16)
                OffsetText = $property.Name
                Relative = $property.Value
                Path = (Join-Path $BuildDir $property.Value)
            }
        }
    }
    else {
        throw "Не найден ни $argsFile, ни $jsonFile. Соберите прошивку, потом повторите."
    }

    if ($parts.Count -eq 0) { throw "В $argsFile не разобрано ни одной части образа." }

    $parts = @($parts | Sort-Object Offset)
    foreach ($part in $parts) {
        if (-not (Test-Path -LiteralPath $part.Path)) {
            throw "Нет файла $($part.Path) (адрес $($part.OffsetText)). Пересоберите прошивку."
        }
        $part | Add-Member -NotePropertyName Size -NotePropertyValue (Get-Item -LiteralPath $part.Path).Length
    }

    return [pscustomobject]@{
        FlashMode = $flashMode
        FlashFreq = $flashFreq
        FlashSize = $flashSize
        Parts     = $parts
        Source    = $(if (Test-Path -LiteralPath $argsFile) { $argsFile } else { $jsonFile })
    }
}

# ---------------------------------------------------------------------------
# 2. esptool из окружения ESP-IDF.
# ---------------------------------------------------------------------------

function Get-EsptoolCommand {
    <#
        Возвращает @{ File; Prefix } — то, что нужно склеить с аргументами:
        либо путь к esptool.exe, либо python с esptool.py / -m esptool.
        Ищет там же, где лежит idf.py: python_env в IDF_TOOLS_PATH.
    #>
    [CmdletBinding()]
    param()

    $pythonEnvRoots = @()
    if ($env:IDF_PYTHON_ENV_PATH) { $pythonEnvRoots += $env:IDF_PYTHON_ENV_PATH }
    $toolsPath = if ($env:IDF_TOOLS_PATH) { $env:IDF_TOOLS_PATH } else { Join-Path $HOME '.espressif' }
    $pythonEnvDir = Join-Path $toolsPath 'python_env'
    if (Test-Path -LiteralPath $pythonEnvDir) {
        $pythonEnvRoots += @(Get-ChildItem -LiteralPath $pythonEnvDir -Directory -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending |
            ForEach-Object { $_.FullName })
    }

    foreach ($root in $pythonEnvRoots) {
        $exe = Join-Path $root 'Scripts\esptool.exe'
        if (Test-Path -LiteralPath $exe) { return [pscustomobject]@{ File = $exe; Prefix = @() } }
    }

    $onPath = Get-Command 'esptool' -ErrorAction SilentlyContinue
    if ($onPath) {
        $file = if ($onPath.Source) { $onPath.Source } else { $onPath.Path }
        return [pscustomobject]@{ File = $file; Prefix = @() }
    }
    $onPathPy = Get-Command 'esptool.py' -ErrorAction SilentlyContinue
    if ($onPathPy) {
        $file = if ($onPathPy.Source) { $onPathPy.Source } else { $onPathPy.Path }
        return [pscustomobject]@{ File = $file; Prefix = @() }
    }

    # Последний вариант: esptool.py, вложенный в сам ESP-IDF, с любым python.
    $python = $null
    foreach ($root in $pythonEnvRoots) {
        $candidate = Join-Path $root 'Scripts\python.exe'
        if (Test-Path -LiteralPath $candidate) { $python = $candidate; break }
    }
    if (-not $python) {
        $onPathPython = Get-Command 'python' -ErrorAction SilentlyContinue
        if ($onPathPython) { $python = $onPathPython.Source }
    }
    if ($python) {
        if ($env:IDF_PATH) {
            $vendored = Join-Path $env:IDF_PATH 'components\esptool_py\esptool\esptool.py'
            if (Test-Path -LiteralPath $vendored) {
                return [pscustomobject]@{ File = $python; Prefix = @($vendored) }
            }
        }
        return [pscustomobject]@{ File = $python; Prefix = @('-m', 'esptool') }
    }

    return $null
}

function Initialize-IdfForEsptool {
    <# Экспортирует окружение ESP-IDF, если esptool не нашёлся сразу. #>
    if (Get-Command Initialize-IdfEnvironment -ErrorAction SilentlyContinue) {
        Write-Step '==> esptool не найден сразу, экспортирую окружение ESP-IDF...'
        Initialize-IdfEnvironment -RepoRoot $repoRoot | Out-Null
    }
}

function Get-EsptoolMajor {
    param([Parameter(Mandatory)] $Tool)
    $raw = (& $Tool.File @($Tool.Prefix + @('version')) 2>&1 | Out-String)
    if ($raw -match 'v?(\d+)\.\d+') { return [int]$Matches[1] }
    return 4
}

# ---------------------------------------------------------------------------
# 3. Сборка образа.
# ---------------------------------------------------------------------------

Write-Step "==> Части образа: $BuildDir"

$image = Read-FlashParts -BuildDir $BuildDir

if ($image.Parts[0].Offset -ne 0) {
    throw ("Самая младшая часть образа лежит по адресу $($image.Parts[0].OffsetText), а манифест " +
           'записывает образ с 0x0. Проверьте build/flash_args.')
}

Write-Host ("    {0,-10} {1,-34} {2,10}" -f 'адрес', 'файл', 'байт')
foreach ($part in $image.Parts) {
    Write-Host ("    {0,-10} {1,-34} {2,10}" -f $part.OffsetText, $part.Relative, $part.Size)
}
Write-Host ("    режим flash: {0}, частота: {1}, размер: {2}" -f $image.FlashMode, $image.FlashFreq, $image.FlashSize)

$tool = Get-EsptoolCommand
if (-not $tool) {
    Initialize-IdfForEsptool
    $tool = Get-EsptoolCommand
}
if (-not $tool) {
    Write-Host 'ERROR: esptool не найден.' -ForegroundColor Red
    Write-Host 'Ожидался $IDF_TOOLS_PATH\python_env\<env>\Scripts\esptool.exe рядом с idf.py.'
    Write-Host 'Проверьте .idf-env.json (toolsPath) или установите ESP-IDF.'
    exit 3
}

$major = Get-EsptoolMajor -Tool $tool
# esptool 5 переименовал команду и ключи в дефисные: merge-bin --flash-mode.
if ($major -ge 5) {
    $subcommand = 'merge-bin'
    $optionNames = @{ mode = '--flash-mode'; freq = '--flash-freq'; size = '--flash-size' }
} else {
    $subcommand = 'merge_bin'
    $optionNames = @{ mode = '--flash_mode'; freq = '--flash_freq'; size = '--flash_size' }
}

$outputDir = Split-Path -Parent $Output
if ($outputDir -and -not (Test-Path -LiteralPath $outputDir)) {
    New-Item -ItemType Directory -Force -Path $outputDir | Out-Null
}

$esptoolArgs = @($tool.Prefix) + @('--chip', $Chip, $subcommand, '-o', $Output)
# -t 0: байт 0 образа — это адрес 0 flash, как и ожидает манифест.
$esptoolArgs += @('-t', '0')
if ($image.FlashMode) { $esptoolArgs += @($optionNames.mode, $image.FlashMode) }
if ($image.FlashFreq) { $esptoolArgs += @($optionNames.freq, $image.FlashFreq) }
if ($image.FlashSize) { $esptoolArgs += @($optionNames.size, $image.FlashSize) }
foreach ($part in $image.Parts) {
    $esptoolArgs += @($part.OffsetText, (Join-Path $BuildDir $part.Relative))
}

Write-Step "==> esptool: $($tool.File)"
Write-Host "    $($tool.File) $($esptoolArgs -join ' ')"

& $tool.File @esptoolArgs
if ($LASTEXITCODE -ne 0) {
    Write-Host "ERROR: esptool завершился с кодом $LASTEXITCODE." -ForegroundColor Red
    exit $LASTEXITCODE
}

if (-not (Test-Path -LiteralPath $Output)) {
    Write-Host "ERROR: esptool не создал $Output." -ForegroundColor Red
    exit 4
}

$merged = Get-Item -LiteralPath $Output
$lastPart = $image.Parts[$image.Parts.Count - 1]
$expected = $lastPart.Offset + $lastPart.Size

Write-Host ''
Write-Host ("==> Готово: {0}" -f $merged.FullName) -ForegroundColor Green
Write-Host ("    размер {0} байт ({1:N2} МиБ), конец образа 0x{2:X}" -f `
    $merged.Length, ($merged.Length / 1MB), $merged.Length)
Write-Host ("    последняя часть: {0} по адресу {1} (+{2} байт)" -f `
    $lastPart.Relative, $lastPart.OffsetText, $lastPart.Size)

if ($merged.Length -lt $expected) {
    Write-Warn "    ВНИМАНИЕ: образ короче, чем конец последней части. Проверьте результат."
}
elseif ($merged.Length -gt $expected) {
    Write-Host ("    хвост после последней части: {0} байт" -f ($merged.Length - $expected))
}

# ---------------------------------------------------------------------------
# 4. Манифест: версия, размер и SHA-256 собранного образа.
#
# Размер и хеш лежат в манифесте, а не берутся страницей из заголовка ответа.
# Заголовок Content-Length описывает то, что сервер отдаёт по сети, и на GitHub
# Pages это сжатый поток: страница показывала 701 697 байт вместо 1 176 144.
# Манифест же собирается из самого файла, поэтому число на странице и число
# скачанных байт не могут разойтись.
#
# Поле image повторяет путь к образу из builds: страница берёт путь, размер и
# хеш из одного места, а tools/image-meta-check.js сверяет его с builds и с
# файлом на диске, чтобы повтор пути не стал вторым источником правды.
# ---------------------------------------------------------------------------

$appPart = $image.Parts[$image.Parts.Count - 1]
$buildTime = (Get-Item -LiteralPath $appPart.Path).LastWriteTime
if ($Version) {
    $version = $Version
    Write-Host ("    версия для манифеста: {0} (задана ключом -Version)" -f $version)
}
else {
    $version = $buildTime.ToString('yyyy.MM.dd-HHmm')
    Write-Host ("    версия для манифеста: {0} (по времени сборки {1})" -f $version, $appPart.Relative)
}

if (-not $SkipManifest) {
    $manifestPath = Join-Path $scriptDir 'manifest.json'
    if (Test-Path -LiteralPath $manifestPath) {
        $imageSize = $merged.Length
        $imageHash = (Get-FileHash -LiteralPath $merged.FullName -Algorithm SHA256).Hash
        $mergedName = $merged.FullName.Substring($scriptDir.Length).TrimStart('\', '/') -replace '\\', '/'

        try {
            $old = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
        }
        catch {
            Write-Warn ("    manifest.json не разбирается: {0} — оставил как есть" -f $_.Exception.Message)
            $old = $null
        }

        if ($old) {
            $builds = @()
            foreach ($b in @($old.builds)) {
                $parts = @()
                foreach ($p in @($b.parts)) {
                    $parts += [ordered]@{ path = $p.path; offset = $p.offset }
                }
                $builds += [ordered]@{ chipFamily = $b.chipFamily; parts = $parts }
            }

            $manifest = [ordered]@{
                name                    = $old.name
                version                 = $version
                new_install_prompt_erase = $old.new_install_prompt_erase
                image                   = [ordered]@{
                    path   = $mergedName
                    size   = $imageSize
                    sha256 = $imageHash
                }
                builds                  = $builds
            }

            if ($old.PSObject.Properties.Name -contains 'funding_url') {
                $manifest['funding_url'] = $old.funding_url
            }

            $text = ($manifest | ConvertTo-Json -Depth 12) + "`n"
            $before = Get-Content -LiteralPath $manifestPath -Raw
            if ($before -ne $text) {
                Set-Content -LiteralPath $manifestPath -Value $text -NoNewline -Encoding UTF8
                Write-Host ("    manifest.json: version {0}, {1} байт, SHA-256 {2}" -f `
                    $version, $imageSize, $imageHash.Substring(0, 8))
            }
            else {
                Write-Host ("    manifest.json: уже {0}, {1} байт" -f $version, $imageSize)
            }
        }
    }
}

# ---------------------------------------------------------------------------
# 5. Список изменений для страниц.
#
# Источник один — CHANGELOG.md, и собирает из него список отдельный скрипт
# tools\make-changelog-json.ps1: в файле лежит история всех версий, а не только
# последней. Страница записи берёт из неё первую версию, страница истории — все.
# ---------------------------------------------------------------------------

if (-not $SkipManifest) {
    $changelogScript = [IO.Path]::GetFullPath((Join-Path $scriptDir '..\make-changelog-json.ps1'))

    if (-not (Test-Path -LiteralPath $changelogScript)) {
        Write-Warn ("    нет {0}, changelog.json не тронут" -f $changelogScript)
    }
    else {
        try {
            & $changelogScript | ForEach-Object { Write-Host ("    {0}" -f $_) }
        }
        catch {
            Write-Warn ("    список изменений не собран: {0}" -f $_.Exception.Message)
        }
    }
}

exit 0
