# Swarm Deployment Script

# 1. Start Orchestrator (on Rack or Main PC)
# Set your Neo4j password if different
$env:NEO4J_PASSWORD="agentic_secure_password"
# Start the orchestrator
go run ./cmd/orchestrator

# 2. Start Node Agent (on any PC/Rack)
# Set the list of orchestrators (Wireguard IPs or hostnames)
$env:ORCHESTRATOR_ENDPOINTS="localhost:50052,10.0.0.1:50052"
# Set the tier for this machine ("PC" or "Rack")
$env:NODE_TIER="PC"
# Start the node agent
go run ./cmd/node_agent

# 3. For Local Testing (Autonomous Loop)
# Set DEBUG_MODE="true" to use file-based loop instead of LM Studio
$env:DEBUG_MODE="true"
go run ./cmd/node_agent
