# Treeline CLI Installer for Windows
# Usage: irm https://treeline.money/install.ps1 | iex
#
# Installs the Treeline CLI to ~/.treeline/bin/tl.exe

$ErrorActionPreference = "Stop"

$Repo = "treeline-money/treeline"
$InstallDir = "$env:USERPROFILE\.treeline\bin"
$BinaryName = "tl.exe"
$Artifact = "tl-windows-x64.exe"

function Write-ColorOutput($ForegroundColor) {
    $fc = $host.UI.RawUI.ForegroundColor
    $host.UI.RawUI.ForegroundColor = $ForegroundColor
    if ($args) {
        Write-Output $args
    }
    $host.UI.RawUI.ForegroundColor = $fc
}

function Get-LatestVersion {
    try {
        $release = Invoke-RestMethod -Uri "https://api.github.com/repos/$Repo/releases/latest"
        return $release.tag_name
    }
    catch {
        Write-ColorOutput Red "Error: Could not determine latest version"
        Write-Output $_.Exception.Message
        exit 1
    }
}

function Install-TreelineCLI {
    Write-ColorOutput Green "Installing Treeline CLI..."
    Write-Output ""

    $Version = Get-LatestVersion
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$Artifact"
    $DestPath = Join-Path $InstallDir $BinaryName

    Write-Output "  Platform: Windows (x64)"
    Write-Output "  Version:  $Version"
    Write-Output "  Install:  $DestPath"
    Write-Output ""

    # Create install directory
    if (-not (Test-Path $InstallDir)) {
        New-Item -ItemType Directory -Path $InstallDir -Force | Out-Null
    }

    Write-ColorOutput Yellow "Downloading..."

    # Download binary
    try {
        Invoke-WebRequest -Uri $DownloadUrl -OutFile $DestPath -UseBasicParsing
    }
    catch {
        Write-ColorOutput Red "Error: Download failed"
        Write-Output $_.Exception.Message
        exit 1
    }

    Write-ColorOutput Green "Installed successfully!"
    Write-Output ""

    # Check if in PATH
    $UserPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($UserPath -notlike "*$InstallDir*") {
        Write-ColorOutput Yellow "Adding to PATH..."
        $NewPath = "$InstallDir;$UserPath"
        [Environment]::SetEnvironmentVariable("Path", $NewPath, "User")
        Write-Output ""
        Write-Output "Added $InstallDir to your PATH."
        Write-Output "Please restart your terminal for the change to take effect."
        Write-Output ""
    }

    Write-Output "Run 'tl --help' to get started."
}

Install-TreelineCLI
