# Usage: .\scripts\release.ps1 -Branch feat/for-loops -Version 0.25.0-alpha -Message "for loops"
param(
    [Parameter(Mandatory)] [string]$Branch,
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [string]$Message
)

function Run([string]$Command) {
    Write-Host "> $Command"
    Invoke-Expression $Command
    if ($LASTEXITCODE -ne 0) { throw "failed: $Command" }
}

if (git status --porcelain) { throw "working tree is not clean; commit or stash first" }
Run "git rev-parse --verify $Branch"
Run "git fetch origin"

# Refuse to continue if local dev/main have diverged from origin.
foreach ($b in 'dev', 'main') {
    Run "git checkout $b"
    Run "git merge --ff-only origin/$b"
}

# Test the feature branch before anything is merged or pushed.
Run "git checkout $Branch"
Run "cargo test"
Run ".\scripts\check-examples.ps1"

Run "git checkout dev"
Run "git merge --no-ff $Branch -m `"merge: $Branch into dev`""
Run "git push -u origin dev"
Run "git push -u origin $Branch"

Run "git checkout main"
Run "git merge --no-ff dev -m `"release: merge dev into main for v$Version`""
Run "git push origin main"
Run "git tag -a v$Version -m `"v${Version}: $Message`""
Run "git push origin v$Version"
Run "git checkout dev"