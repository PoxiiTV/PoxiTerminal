[CmdletBinding()]
param(
    [ValidatePattern('^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$')]
    [string] $Version,

    # 与便携包相同，安装器只能收录已内嵌 GPUI 着色器的发布构建。
    [ValidateSet('release')]
    [string] $Configuration = 'release',

    [ValidateSet('PoxiTerminal')]
    [string] $PackageBrand = 'PoxiTerminal',

    [ValidateSet('x64', 'arm64')]
    [string] $Architecture = 'x64',

    [switch] $SkipBuild,
    # 与 -SkipBuild 联用：跳过「exe 必须比源码新」的陈旧检查。仅用于脚本
    # 自测；发布安装包一律走全新构建。
    [switch] $AllowStale,
    [switch] $Force,
    [switch] $ValidateOnly,
    [string] $OutputDirectory,
    [string] $TargetDirectory,
    [string] $InnoCompiler
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$manifestPath = Join-Path $repo 'nebula_app\Cargo.toml'
$installerScript = Join-Path $PSScriptRoot 'installer.iss'

if ([string]::IsNullOrWhiteSpace($Version)) {
    $cargoManifest = Get-Content -LiteralPath $manifestPath -Raw -Encoding UTF8
    $match = [regex]::Match($cargoManifest, '(?m)^version\s*=\s*"(?<version>[^"]+)"\s*$')
    if (-not $match.Success) {
        throw "Unable to read the package version from $manifestPath"
    }
    $Version = $match.Groups['version'].Value
}

if ($Version -notmatch '^(?<major>[0-9]+)\.(?<minor>[0-9]+)\.(?<patch>[0-9]+)') {
    throw "Version must begin with three numeric components: $Version"
}
$numericVersion = "$($Matches.major).$($Matches.minor).$($Matches.patch).0"

if ([string]::IsNullOrWhiteSpace($OutputDirectory)) {
    $OutputDirectory = Join-Path $repo 'dist'
} elseif (-not [System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $OutputDirectory = Join-Path $repo $OutputDirectory
}
$outputRoot = [System.IO.Path]::GetFullPath($OutputDirectory)
if ([string]::IsNullOrWhiteSpace($TargetDirectory)) {
    $TargetDirectory = Join-Path $repo 'target'
} elseif (-not [System.IO.Path]::IsPathRooted($TargetDirectory)) {
    $TargetDirectory = Join-Path $repo $TargetDirectory
}
$cargoTargetRoot = [System.IO.Path]::GetFullPath($TargetDirectory)
$targetRoot = Join-Path $cargoTargetRoot $Configuration
$setupPath = Join-Path $outputRoot "$PackageBrand-v$Version-windows-$Architecture-setup.exe"

$requiredFiles = @(
    (Join-Path $targetRoot 'poxiterminal.exe'),
    (Join-Path $targetRoot 'poxiterminal-hook.exe'),
    (Join-Path $targetRoot 'conpty.dll'),
    (Join-Path $targetRoot 'OpenConsole.exe'),
    (Join-Path $repo 'README.md'),
    (Join-Path $repo 'INSTALL.md'),
    (Join-Path $repo 'docs\lua-configuration.md'),
    (Join-Path $repo 'docs\runtime-control-api.md'),
    (Join-Path $repo 'docs\runtime-api-v1.schema.json'),
    (Join-Path $repo 'docs\skills\poxiterminal-runtime\SKILL.md'),
    (Join-Path $repo 'docs\skills\poxiterminal-runtime\agents\openai.yaml'),
    (Join-Path $repo 'assets\fonts\MapleMonoNormal-NF-CN-Regular.ttf'),
    (Join-Path $repo 'LICENSE'),
    (Join-Path $repo 'licenses\LICENSE-LUA'),
    (Join-Path $repo 'licenses\LICENSE-MLUA'),
    (Join-Path $repo 'THIRD-PARTY-NOTICES')
)

if (-not (Test-Path -LiteralPath $installerScript -PathType Leaf)) {
    throw "Installer definition is missing: $installerScript"
}

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'build-windows-product.ps1') -Configuration $Configuration -TargetDirectory $cargoTargetRoot
}

$missing = @($requiredFiles | Where-Object { -not (Test-Path -LiteralPath $_ -PathType Leaf) })
if ($missing.Count -ne 0) {
    throw "Required installer files are missing:`n$($missing -join "`n")"
}

$packagedExe = Join-Path $targetRoot 'poxiterminal.exe'
. (Join-Path $PSScriptRoot 'windows-package-architecture.ps1')
Assert-WindowsPackageArchitecture -Root $targetRoot -Architecture $Architecture
if ((Get-Item -LiteralPath $packagedExe).VersionInfo.ProductName -ne 'PoxiTerminal') {
    throw 'PoxiTerminal packages require a freshly built PoxiTerminal executable.'
}
# 判据同 package-release.ps1：二进制不得早于其源码最新改动；cargo 对未
# 变更目标不重链接，不能拿运行开始时刻当基准。
if (-not $AllowStale) {
    $memberDirs = @(
        'nebula_app', 'nebula_terminal', 'nebula_config', 'nebula_config_derive',
        'nebula-completions', 'nebula_gpui', 'nebula_settings', 'nebula_split'
    ) | ForEach-Object { Join-Path $repo $_ }
    $sourceScopes = @(
        @{ Binary = $packagedExe; Roots = $memberDirs + @(
            (Join-Path $repo 'Cargo.toml'), (Join-Path $repo '..\gpui-component-fork\crates')) },
        @{ Binary = (Join-Path $targetRoot 'poxiterminal-hook.exe'); Roots = @(
            (Join-Path $repo 'nebula_hook'), (Join-Path $repo 'Cargo.toml')) }
    )
    foreach ($scope in $sourceScopes) {
        $newestSource = [datetime]::MinValue
        foreach ($root in $scope.Roots) {
            if (-not (Test-Path -LiteralPath $root)) { continue }
            if (Test-Path -LiteralPath $root -PathType Leaf) {
                $times = @((Get-Item -LiteralPath $root).LastWriteTime)
            } else {
                $times = Get-ChildItem -LiteralPath $root -Recurse -File -ErrorAction SilentlyContinue |
                    Where-Object { $_.Extension -in '.rs', '.toml' } |
                    ForEach-Object { $_.LastWriteTime }
            }
            foreach ($time in @($times)) {
                if ($time -gt $newestSource) { $newestSource = $time }
            }
        }
        $item = Get-Item -LiteralPath $scope.Binary
        if ($item.LastWriteTime -lt $newestSource) {
            throw "Stale binary: $($scope.Binary) ($($item.LastWriteTime)) is older than the newest source change ($newestSource). Rebuild before packaging, or pass -AllowStale if you really mean it."
        }
    }
}
$helpText = & $packagedExe --help 2>&1 | Out-String
if ($helpText -notmatch '--gpui') {
    throw "poxiterminal.exe at $packagedExe is the legacy shell (no --gpui in --help). Rebuild with --features gpui-shell; do not package a workspace-default binary."
}
$versionText = & $packagedExe --version 2>&1 | Out-String
if ($versionText -notmatch [regex]::Escape($Version)) {
    throw "poxiterminal.exe reports `"$($versionText.Trim())`" but the installer version is $Version. The staged exe does not match this release."
}

if ($ValidateOnly) {
    [PSCustomObject]@{
        InstallerScript = $installerScript
        Version = $Version
        Configuration = $Configuration
        Architecture = $Architecture
        Files = $requiredFiles.Count
    } | Format-List
    return
}

if (Test-Path -LiteralPath $setupPath) {
    if (-not $Force) {
        throw "Installer already exists: $setupPath (pass -Force to replace it)"
    }
    Remove-Item -LiteralPath $setupPath -Force
}
New-Item -ItemType Directory -Path $outputRoot -Force | Out-Null

if ([string]::IsNullOrWhiteSpace($InnoCompiler)) {
    $candidates = @(
        $env:ISCC_PATH,
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe')
    ) | Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    $InnoCompiler = $candidates | Where-Object {
        Test-Path -LiteralPath $_ -PathType Leaf
    } | Select-Object -First 1
}
if ([string]::IsNullOrWhiteSpace($InnoCompiler) -or
    -not (Test-Path -LiteralPath $InnoCompiler -PathType Leaf)) {
    throw 'ISCC.exe was not found. Install Inno Setup 6 or pass -InnoCompiler / set ISCC_PATH.'
}

Push-Location $PSScriptRoot
try {
    & $InnoCompiler "/DAppVersion=$Version" "/DNumericVersion=$numericVersion" "/DConfiguration=$Configuration" "/DPackageBrand=$PackageBrand" "/DArchitecture=$Architecture" "/DBuildRoot=$targetRoot" "/O$outputRoot" $installerScript
    if ($LASTEXITCODE -ne 0) {
        throw "Inno Setup compilation failed with exit code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}

if (-not (Test-Path -LiteralPath $setupPath -PathType Leaf)) {
    throw "Inno Setup did not create the expected installer: $setupPath"
}

$setup = Get-Item -LiteralPath $setupPath
[PSCustomObject]@{
    Path = $setup.FullName
    Size = $setup.Length
    SHA256 = (Get-FileHash -LiteralPath $setupPath -Algorithm SHA256).Hash
} | Format-List
