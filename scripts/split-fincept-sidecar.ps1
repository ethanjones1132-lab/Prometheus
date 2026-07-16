# Sprint 7.1 — prepare a clean fincept-sidecar subtree branch for public push.
# Does NOT push to a remote (operator supplies remote URL).
#
# Usage (from monorepo root):
#   .\scripts\split-fincept-sidecar.ps1
#   .\scripts\split-fincept-sidecar.ps1 -RemoteUrl git@github.com:ORG/fincept-sidecar.git

param(
    [string]$RemoteUrl = "",
    [string]$BranchName = "sidecar-public"
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not (Test-Path (Join-Path $root "fincept-sidecar\main.py"))) {
    throw "fincept-sidecar/main.py not found under $root"
}

Push-Location $root
try {
    Write-Host "Creating subtree split branch '$BranchName' from fincept-sidecar/ ..."
    git subtree split --prefix=fincept-sidecar -b $BranchName
    Write-Host ""
    Write-Host "Branch '$BranchName' ready."
    Write-Host "To publish:"
    Write-Host "  git checkout $BranchName"
    if ($RemoteUrl) {
        Write-Host "  git remote add sidecar-public $RemoteUrl   # if not already added"
        Write-Host "  git push -u sidecar-public ${BranchName}:main"
    } else {
        Write-Host "  git remote add sidecar-public <PUBLIC_REPO_URL>"
        Write-Host "  git push -u sidecar-public ${BranchName}:main"
    }
    Write-Host ""
    Write-Host "Then pin the public commit SHA in kalshi-monster release notes + binaries/README.md"
} finally {
    Pop-Location
}
