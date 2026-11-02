param(
    [Parameter(Mandatory)][string]$Thumbprint,
    [Parameter(Mandatory)][string]$Path,
    [string]$TimestampUrl = 'http://timestamp.digicert.com'
)

$ErrorActionPreference = 'Stop'
Import-Module (Join-Path $PSHOME 'Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1')

$signtool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" |
    Where-Object { $_.Directory.Parent.Name -as [version] } |
    Sort-Object { [version]$_.Directory.Parent.Name } |
    Select-Object -Last 1
if (-not $signtool) {
    throw 'signtool.exe not found: install the Windows SDK'
}

& $signtool.FullName sign /sha1 $Thumbprint /fd SHA256 /tr $TimestampUrl /td SHA256 /d Octo $Path
if ($LASTEXITCODE -ne 0) {
    throw "signtool could not sign $Path with the certificate $Thumbprint"
}

$signature = Get-AuthenticodeSignature -LiteralPath $Path
if ($signature.SignerCertificate.Thumbprint -ne $Thumbprint) {
    throw "$Path is not signed with the certificate $Thumbprint"
}
if (-not $signature.TimeStamperCertificate) {
    throw "$Path has no timestamp from $TimestampUrl"
}
