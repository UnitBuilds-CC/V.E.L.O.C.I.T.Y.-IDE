# Start-Orchestrator.ps1
# Helper script to start the Orchestrator on the Dev PC

$env:VAULT_PASSWORD = "your_master_password" # User should set this
$env:VAULT_AUTO_APPROVE = "true" # Development flag
$env:ORCHESTRATOR_ADDR = "localhost:50052"
$env:NODE_ENDPOINT = "10.0.0.1:50051"

Write-Host "Starting Swarm Orchestrator on Dev PC..." -ForegroundColor Cyan
Write-Host "Listening on all interfaces (Port 50052)" -ForegroundColor Yellow

go run ./cmd/orchestrator
