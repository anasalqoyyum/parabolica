param([string]$Destination = (Join-Path $env:LOCALAPPDATA 'Parabolica-build\asiosdk-2.3.4'))
$ErrorActionPreference = 'Stop'
$expected = 'D5EBF0C20DD2C5F43771FD0C1418F4B361BF52434EE670097CFA6B3A335E2ECA'
$header = Join-Path $Destination 'ASIOSDK\common\asio.h'
if (!(Test-Path $header)) {
    $archive = Join-Path ([IO.Path]::GetTempPath()) ('parabolica-asiosdk-' + [Guid]::NewGuid().ToString() + '.zip')
    try {
        Invoke-WebRequest 'https://www.steinberg.net/asiosdk' -OutFile $archive -UseBasicParsing
        if ((Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected) {
            throw 'ASIO SDK archive changed. Expected SDK 2.3.4; do not build with unreviewed SDK contents.'
        }
        if (Test-Path $Destination) { throw "Destination exists without its SDK header: $Destination. Choose a new destination." }
        Expand-Archive $archive -DestinationPath $Destination
    } finally {
        Remove-Item $archive -ErrorAction SilentlyContinue
    }
}
$env:PARABOLICA_ASIO_SDK_DIR = Join-Path $Destination 'ASIOSDK'
if ($env:GITHUB_ENV) { "PARABOLICA_ASIO_SDK_DIR=$env:PARABOLICA_ASIO_SDK_DIR" | Out-File $env:GITHUB_ENV -Append -Encoding utf8 }
Write-Output "ASIO SDK: $env:PARABOLICA_ASIO_SDK_DIR"
Write-Output 'SDK 2.3.4 is dual licensed under GPLv3 or the proprietary Steinberg ASIO license. See docs/implementation/asio.md.'
