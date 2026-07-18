# Start-Agent.ps1
# Helper script to start the Node Agent on the Laptop

$env:ORCHESTRATOR_ADDR = "10.0.0.1:50052"
$env:NODE_ENDPOINT = "10.0.0.2:50051"
$env:DEBUG_MODE = "false"
$env:LM_STUDIO_URL = "http://10.0.0.1:1234/v1" # Point to Dev PC's model
$env:NEO4J_URI = "bolt://10.0.0.1:7687"
$env:NEO4J_USER = "neo4j"
$env:NEO4J_PASSWORD = "agentic_secure_password"

Write-Host "Starting Swarm Node Agent on Laptop..." -ForegroundColor Cyan
Write-Host "Connecting to Orchestrator at $env:ORCHESTRATOR_ADDR" -ForegroundColor Yellow

go run ./cmd/node_agent
