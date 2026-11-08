param(
    [Parameter(Mandatory)][string]$Binary,
    [string]$Identity = 'OctoAI.OctoAIAgents',
    [string]$Publisher = 'CN=7C9FFDFE-038C-491B-903D-40B89EE3BD87',
    [string]$PublisherDisplayName = 'OctoAI',
    [string]$DisplayName,
    [string]$Layout,
    [string]$Out
)

$ErrorActionPreference = 'Stop'
if (-not $DisplayName) {
    $DisplayName = "Octo $([char]0x2014) AI Agents"
}
if (-not $Layout) {
    $Layout = Join-Path $PSScriptRoot '..\target\msix'
}
if (-not $Out) {
    $Out = Join-Path $PSScriptRoot '..\target\installer\Octo.msix'
}

function Find-SdkTool([string]$Name) {
    $tool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\$Name" |
        Where-Object { $_.Directory.Parent.Name -as [version] } |
        Sort-Object { [version]$_.Directory.Parent.Name } |
        Select-Object -Last 1
    if (-not $tool) {
        throw "$Name not found: install the Windows SDK"
    }
    $tool.FullName
}

function Escape-Xml([string]$Text) {
    [Security.SecurityElement]::Escape($Text)
}

$productVersion = (Get-Item -LiteralPath $Binary).VersionInfo.ProductVersion
if ($productVersion -notmatch '^(\d+)\.(\d+)\.(\d+)$') {
    throw "$Binary has no ProductVersion like 1.2.3: build it with tauri-build"
}
$version = "$productVersion.0"

$makeappx = Find-SdkTool 'makeappx.exe'
$makepri = Find-SdkTool 'makepri.exe'
$source = Join-Path $PSScriptRoot 'msix'

if (Test-Path -LiteralPath $Layout) {
    Remove-Item -LiteralPath $Layout -Recurse -Force
}
New-Item -ItemType Directory -Path $Layout | Out-Null
Copy-Item -LiteralPath (Join-Path $source 'Assets') -Destination $Layout -Recurse
Copy-Item -LiteralPath $Binary -Destination (Join-Path $Layout 'Octo.exe')

$manifest = (Get-Content -LiteralPath (Join-Path $source 'AppxManifest.xml') -Raw -Encoding UTF8).
    Replace('{Identity}', (Escape-Xml $Identity)).
    Replace('{Publisher}', (Escape-Xml $Publisher)).
    Replace('{Version}', $version).
    Replace('{DisplayName}', (Escape-Xml $DisplayName)).
    Replace('{PublisherDisplayName}', (Escape-Xml $PublisherDisplayName))
[IO.File]::WriteAllText((Join-Path $Layout 'AppxManifest.xml'), $manifest, (New-Object Text.UTF8Encoding $false))

$priConfig = Join-Path ([IO.Path]::GetTempPath()) "octo-priconfig-$PID.xml"
try {
    & $makepri createconfig /cf $priConfig /dq en-US /pv 10.0.0 /o | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'makepri could not create its configuration'
    }
    & $makepri new /pr $Layout /cf $priConfig /mn (Join-Path $Layout 'AppxManifest.xml') /of (Join-Path $Layout 'resources.pri') /o | Out-Null
    if ($LASTEXITCODE -ne 0) {
        throw 'makepri could not index the assets'
    }
} finally {
    Remove-Item -LiteralPath $priConfig -ErrorAction SilentlyContinue
}

New-Item -ItemType Directory -Path (Split-Path $Out) -Force | Out-Null
& $makeappx pack /d $Layout /p $Out /o | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "makeappx could not pack $Layout"
}
Write-Output $Out
