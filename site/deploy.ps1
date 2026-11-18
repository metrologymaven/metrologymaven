# Build and deploy metrologymaven.io from this machine (needs `npx wrangler@4 login` once).
# CI deploys automatically once the CLOUDFLARE_API_TOKEN repo secret is set (.github/workflows/site.yml).
$ErrorActionPreference = "Stop"
Push-Location $PSScriptRoot
try {
    python build.py
    if ($LASTEXITCODE -ne 0) { throw "site build failed" }
    npx --yes wrangler@4 deploy
    if ($LASTEXITCODE -ne 0) { throw "wrangler deploy failed" }
} finally {
    Pop-Location
}
