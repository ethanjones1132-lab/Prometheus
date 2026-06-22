# Agent healthcheck for kalshi-monster
# Run from repo root: .\scripts\agent-healthcheck.ps1
# Exit 0 = all checks passed; non-zero = first failure

$ErrorActionPreference = "Stop"
$RepoRoot = Split-Path -Parent $PSScriptRoot
$Failed = $false

function Step($label, [scriptblock]$Action) {
    Write-Host ""
    Write-Host "== $label ==" -ForegroundColor Cyan
    try {
        $global:LASTEXITCODE = 0
        & $Action
        if ($LASTEXITCODE -ne 0) {
            throw "Exit code $LASTEXITCODE"
        }
        Write-Host "PASS: $label" -ForegroundColor Green
    } catch {
        Write-Host "FAIL: $label" -ForegroundColor Red
        Write-Host $_.Exception.Message -ForegroundColor Red
        $script:Failed = $true
    }
}

Write-Host "kalshi-monster agent healthcheck"
Write-Host "Repo: $RepoRoot"

Step "UI typecheck" {
    Push-Location (Join-Path $RepoRoot "src-ui")
    npm run typecheck
    Pop-Location
}

Step "Rust cargo check" {
    Push-Location (Join-Path $RepoRoot "src-tauri")
    cargo check
    Pop-Location
}

Step "Kalshi Rust tests" {
    Push-Location (Join-Path $RepoRoot "src-tauri")
    cargo test kalshi::
    Pop-Location
}

Step "PRIORITIES.md present" {
    $priorities = Join-Path $RepoRoot "PRIORITIES.md"
    if (-not (Test-Path $priorities)) {
        throw "Missing PRIORITIES.md"
    }
    Get-Content $priorities -TotalCount 5 | ForEach-Object { Write-Host "  $_" }
}

Step "Paper module wired" {
    $lib = Join-Path $RepoRoot "src-tauri\src\lib.rs"
    $content = Get-Content $lib -Raw
    if ($content -notmatch "pub mod paper") {
        throw "pub mod paper missing from lib.rs"
    }
    if ($content -notmatch "init_paper_tables") {
        throw "init_paper_tables not called in lib.rs"
    }
    if ($content -notmatch "paper_get_analytics") {
        throw "paper_get_analytics command not registered in lib.rs"
    }
    $paper = Join-Path $RepoRoot "src-tauri\src\paper\mod.rs"
    if (-not (Test-Path $paper)) {
        throw "src-tauri/src/paper/mod.rs missing"
    }
}

Step "Quick cache path (flat /markets)" {
    $client = Join-Path $RepoRoot "src-tauri\src\kalshi\client.rs"
    $content = Get-Content $client -Raw
    if ($content -notmatch "fetch_markets_flat_pages") {
        throw "fetch_markets_flat_pages not found in client.rs (perf regression risk)"
    }
    if ($content -notmatch "ensure_quick_cache") {
        throw "ensure_quick_cache not found in client.rs"
    }
}

if ($Failed) {
    Write-Host ""
    Write-Host "HEALTHCHECK: FAIL" -ForegroundColor Red
    exit 1
}

Write-Host ""
Write-Host "HEALTHCHECK: PASS" -ForegroundColor Green
exit 0