<#
.SYNOPSIS
    Отдаёт каталог tools/webflash по localhost — для страницы прошивки OBOD.

.DESCRIPTION
    Web Serial работает только в защищённом контексте, поэтому файл нельзя
    открыть двойным щелчком: страницу нужно отдать по http с localhost.
    Этот скрипт поднимает минимальный статический сервер на фиксированном порту
    и печатает точный адрес для браузера.

        pwsh -File tools/webflash/serve.ps1
        pwsh -File tools/webflash/serve.ps1 -Port 8080

    Остановить — Ctrl+C. Ответы отдаются с Cache-Control: no-store, чтобы
    браузер не показывал старый образ после пересборки merge.ps1.

    Слушает только петлевой интерфейс (127.0.0.1 и ::1), наружу не смотрит.

.PARAMETER Port
    Порт. По умолчанию 8787.

.PARAMETER Root
    Каталог для раздачи. По умолчанию — каталог этого скрипта.

.PARAMETER Quiet
    Не печатать каждую отданную страницу.

.EXAMPLE
    pwsh -File tools/webflash/serve.ps1
#>
#Requires -Version 5.1

[CmdletBinding()]
param(
    [int]    $Port  = 8787,
    [string] $Root,
    [switch] $Quiet
)

$ErrorActionPreference = 'Stop'

if (-not $Root) {
    $Root = $PSScriptRoot
    if (-not $Root) { $Root = Split-Path -Parent $MyInvocation.MyCommand.Path }
}
$Root = (Resolve-Path -LiteralPath $Root).Path

function Get-ContentType {
    param([string] $Path)
    switch ([System.IO.Path]::GetExtension($Path).ToLowerInvariant()) {
        '.html' { 'text/html; charset=utf-8' }
        '.htm'  { 'text/html; charset=utf-8' }
        '.json' { 'application/json; charset=utf-8' }
        '.js'   { 'text/javascript; charset=utf-8' }
        '.css'  { 'text/css; charset=utf-8' }
        '.svg'  { 'image/svg+xml' }
        '.png'  { 'image/png' }
        '.jpg'  { 'image/jpeg' }
        '.ico'  { 'image/x-icon' }
        '.txt'  { 'text/plain; charset=utf-8' }
        '.md'   { 'text/plain; charset=utf-8' }
        '.bin'  { 'application/octet-stream' }
        default { 'application/octet-stream' }
    }
}

function Send-Response {
    param(
        [Parameter(Mandatory)] $Client,
        [int]    $Status,
        [string] $Reason,
        [byte[]] $Body,
        [string] $ContentType = 'application/octet-stream',
        [switch] $Head
    )
    if (-not $Body) { $Body = New-Object byte[] 0 }
    $header = "HTTP/1.1 $Status $Reason`r`n" +
              "Content-Type: $ContentType`r`n" +
              "Content-Length: $($Body.Length)`r`n" +
              "Cache-Control: no-store, must-revalidate`r`n" +
              "Pragma: no-cache`r`n" +
              "Connection: close`r`n`r`n"
    $stream = $Client.GetStream()
    $headerBytes = [System.Text.Encoding]::ASCII.GetBytes($header)
    $stream.Write($headerBytes, 0, $headerBytes.Length)
    if (-not $Head -and $Body.Length -gt 0) { $stream.Write($Body, 0, $Body.Length) }
    $stream.Flush()
}

function Send-Text {
    param($Client, [int] $Status, [string] $Reason, [string] $Text, [switch] $Head)
    Send-Response -Client $Client -Status $Status -Reason $Reason `
        -Body ([System.Text.Encoding]::UTF8.GetBytes($Text)) `
        -ContentType 'text/plain; charset=utf-8' -Head:$Head
}

function Read-RequestLine {
    <# Читает заголовки запроса и возвращает первую строку ("GET /path HTTP/1.1"). #>
    param([Parameter(Mandatory)] $Client)
    $stream = $Client.GetStream()
    $stream.ReadTimeout = 5000
    $buffer = New-Object byte[] 8192
    $text = ''
    while ($text.IndexOf("`r`n`r`n") -lt 0) {
        $read = $stream.Read($buffer, 0, $buffer.Length)
        if ($read -le 0) { break }
        $text += [System.Text.Encoding]::ASCII.GetString($buffer, 0, $read)
        if ($text.Length -gt 65536) { break }
    }
    if (-not $text) { return $null }
    return ($text -split "`r`n")[0]
}

function Resolve-RequestPath {
    <# Превращает путь из строки запроса в файл внутри Root. $null — выход за Root. #>
    param([string] $UrlPath, [string] $Root)
    $relative = $UrlPath
    $queryAt = $relative.IndexOf('?')
    if ($queryAt -ge 0) { $relative = $relative.Substring(0, $queryAt) }
    $relative = [System.Uri]::UnescapeDataString($relative)
    $relative = $relative.TrimStart('/').Replace('/', '\')
    if (-not $relative) { $relative = 'index.html' }
    $full = [System.IO.Path]::GetFullPath((Join-Path $Root $relative))
    if (-not $full.StartsWith($Root, [System.StringComparison]::OrdinalIgnoreCase)) { return $null }
    if (Test-Path -LiteralPath $full -PathType Container) {
        $full = Join-Path $full 'index.html'
    }
    return $full
}

# ---------------------------------------------------------------------------
# Слушатели: 127.0.0.1 и, если получится, ::1 — чтобы работали и
# http://localhost:8787/, и http://127.0.0.1:8787/.
# ---------------------------------------------------------------------------

$listenerV4 = $null
$listenerV6 = $null

try {
    $listenerV4 = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::Loopback, $Port)
    $listenerV4.Start()
}
catch {
    Write-Host "ERROR: не удалось занять порт $Port." -ForegroundColor Red
    Write-Host 'Порт занят другой программой — укажите другой:'
    Write-Host "  pwsh -File tools/webflash/serve.ps1 -Port 8080"
    exit 3
}

try {
    $listenerV6 = [System.Net.Sockets.TcpListener]::new([System.Net.IPAddress]::IPv6Loopback, $Port)
    $listenerV6.Start()
}
catch {
    $listenerV6 = $null
}

$firmware = Join-Path $Root 'firmware\OBOD.bin'

Write-Host ''
Write-Host 'Сервер страницы прошивки OBOD' -ForegroundColor Cyan
Write-Host "  каталог:    $Root"
Write-Host "  страница:   http://localhost:$Port/"
Write-Host "  манифест:   http://localhost:$Port/manifest.json"
Write-Host "  образ:      http://localhost:$Port/firmware/OBOD.bin"
if (Test-Path -LiteralPath $firmware) {
    $size = (Get-Item -LiteralPath $firmware).Length
    Write-Host ("  образ на диске: {0:N0} байт" -f $size)
} else {
    Write-Host '  образ не собран: запустите pwsh -File tools/webflash/merge.ps1' -ForegroundColor Yellow
}
Write-Host '  остановить: Ctrl+C'
Write-Host ''

try {
    while ($true) {
        $client = $null
        if ($listenerV4.Pending()) {
            $client = $listenerV4.AcceptTcpClient()
        }
        elseif ($listenerV6 -and $listenerV6.Pending()) {
            $client = $listenerV6.AcceptTcpClient()
        }
        else {
            Start-Sleep -Milliseconds 50
            continue
        }

        try {
            $client.NoDelay = $true
            $requestLine = Read-RequestLine -Client $client
            if (-not $requestLine) { continue }

            $pieces = @($requestLine -split '\s+')
            if ($pieces.Count -lt 2) {
                Send-Text -Client $client -Status 400 -Reason 'Bad Request' -Text 'Bad request'
                continue
            }
            $method = $pieces[0].ToUpperInvariant()
            $target = $pieces[1]
            $isHead = ($method -eq 'HEAD')

            if ($method -ne 'GET' -and -not $isHead) {
                Send-Text -Client $client -Status 405 -Reason 'Method Not Allowed' `
                    -Text 'Только GET и HEAD.' -Head:$isHead
                if (-not $Quiet) { Write-Host "    $method $target -> 405" }
                continue
            }

            $file = Resolve-RequestPath -UrlPath $target -Root $Root
            if (-not $file) {
                Send-Text -Client $client -Status 403 -Reason 'Forbidden' -Text 'Выход за каталог.' -Head:$isHead
                if (-not $Quiet) { Write-Host "    $method $target -> 403" }
                continue
            }

            if (-not (Test-Path -LiteralPath $file -PathType Leaf)) {
                Send-Text -Client $client -Status 404 -Reason 'Not Found' `
                    -Text "Нет файла: $target" -Head:$isHead
                if (-not $Quiet) { Write-Host "    $method $target -> 404" }
                continue
            }

            $bytes = [System.IO.File]::ReadAllBytes($file)
            Send-Response -Client $client -Status 200 -Reason 'OK' -Body $bytes `
                -ContentType (Get-ContentType -Path $file) -Head:$isHead
            if (-not $Quiet) {
                Write-Host ("    {0} {1} -> 200 ({2:N0} байт)" -f $method, $target, $bytes.Length)
            }
        }
        catch {
            if (-not $Quiet) { Write-Host "    запрос не обработан: $($_.Exception.Message)" -ForegroundColor Yellow }
        }
        finally {
            if ($client) { $client.Close() }
        }
    }
}
finally {
    if ($listenerV4) { $listenerV4.Stop() }
    if ($listenerV6) { $listenerV6.Stop() }
    Write-Host ''
    Write-Host 'Сервер остановлен.' -ForegroundColor Cyan
}
