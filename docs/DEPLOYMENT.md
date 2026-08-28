# Deployment Guide

This guide covers deployment Velocity IDE in various environments.

## Table of Contents

- [Prerequisites](#prerequisites)
- [Local Development](#local-development)
- [Docker Deployment](#docker-deployment)
- [Production Deployment](#production-deployment)
- [Configuration](#configuration)
- [Monitoring](#monitoring)

---

## Prerequisites

### System Requirements

**Minimum:**
- CPU: 4 cores
- RAM: 8 GB
- Disk: 2 GB (build), 500 MB (runtime)
- OS: Windows 10+, Ubuntu 20.04+, macOS 12+

**Recommended:**
- CPU: 8+ cores
- RAM: 16+ GB
- Disk: 4+ GB (build), 1+ GB (runtime)
- GPU: Vulkan-capable (for GPU acceleration)

### Software Dependencies

**Build-time:**
- Rust 1.75+ (stable)
- Git
- pkg-config
- libgtk-3-dev (Linux)
- libwebkit2gtk-4.1-dev (Linux)
- libudev-dev (Linux)

**Runtime:**
- Git
- ripgrep (recommended for search)
- Vulkan runtime (optional, for GPU acceleration)

---

## Local Development

### 1. Clone Repository

```bash
git clone https://github.com/UnitBuilds/Velocity-IDE.git
cd Velocity-IDE
```

### 2. Install Dependencies

**Ubuntu/Debian:**
```bash
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    pkg-config \
    libssl-dev \
    libgtk-3-dev \
    libwebkit2gtk-4.1-dev \
    libudev-dev \
    git \
    ripgrep
```

**Windows:**
- Install [Rust](https://rustup.rs/)
- Install [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)
- Install Git for Windows

**macOS:**
```bash
brew install rust git ripgrep
```

### 3. Build

```bash
# Debug build (faster compilation)
cargo build

# Release build (optimized, slower compilation)
cargo build --release
```

### 4. Run Tests

```bash
cargo test --workspace
```

### 5. Run

```bash
# GUI mode
./target/release/velocity_ide

# MCP server mode (headless)
./target/release/velocity_mcp --mode stdio
```

---

## Docker Deployment

### Build Image

```bash
docker build -t velocity-ide:latest .
```

### Run Container

```bash
# Interactive mode with workspace mount
docker run -it \
  -v $(pwd)/workspace:/home/velocity/workspace \
  -e DISPLAY=$DISPLAY \
  -v /tmp/.X11-unix:/tmp/.X11-unix \
  velocity-ide:latest

# Headless MCP server mode
docker run -d \
  -v $(pwd)/workspace:/home/velocity/workspace \
  velocity-ide:latest \
  velocity_mcp --mode stdio
```

### Docker Compose

Create `docker-compose.yml`:

```yaml
version: '3.8'

services:
  velocity-ide:
    build: .
    volumes:
      - ./workspace:/home/velocity/workspace
      - /tmp/.X11-unix:/tmp/.X11-unix
    environment:
      - DISPLAY=$DISPLAY
      - VELOCITY_API_KEY=${VELOCITY_API_KEY}
    ports:
      - "8080:8080"  # If exposing HTTP API
```

Run:
```bash
docker-compose up -d
```

---

## Production Deployment

### 1. Release Build

```bash
# Clean previous builds
cargo clean

# Build optimized release
cargo build --release
```

Binaries are in `target/release/`:
- `velocity_ide` — GUI application
- `velocity_mcp` — MCP server (headless)

### 2. Systemd Service (Linux)

Create `/etc/systemd/system/velocity-mcp.service`:

```ini
[Unit]
Description=Velocity MCP Server
After=network.target

[Service]
Type=simple
User=velocity
Group=velocity
WorkingDirectory=/opt/velocity
ExecStart=/opt/velocity/velocity_mcp --mode stdio
Restart=always
RestartSec=10

# Security hardening
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/opt/velocity/workspace

# Environment
Environment=RUST_LOG=info
Environment=VELOCITY_API_KEY=your-api-key-here

[Install]
WantedBy=multi-user.target
```

Enable and start:
```bash
sudo systemctl daemon-reload
sudo systemctl enable velocity-mcp
sudo systemctl start velocity-mcp
sudo systemctl status velocity-mcp
```

### 3. Windows Service

Use [NSSM](https://nssm.cc/) to create a Windows service:

```powershell
nssm install VelocityMCP "C:\Program Files\Velocity\velocity_mcp.exe"
nssm set VelocityMCP AppParameters "--mode stdio"
nssm set VelocityMCP AppDirectory "C:\Program Files\Velocity"
nssm set VelocityMCP AppEnvironmentExtra "RUST_LOG=info"
nssm start VelocityMCP
```

### 4. Kubernetes Deployment

Create `k8s/deployment.yaml`:

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: velocity-mcp
  labels:
    app: velocity-mcp
spec:
  replicas: 2
  selector:
    matchLabels:
      app: velocity-mcp
  template:
    metadata:
      labels:
        app: velocity-mcp
    spec:
      containers:
      - name: velocity-mcp
        image: your-registry/velocity-ide:latest
        command: ["/usr/local/bin/velocity_mcp"]
        args: ["--mode", "stdio"]
        env:
        - name: RUST_LOG
          value: "info"
        - name: VELOCITY_API_KEY
          valueFrom:
            secretKeyRef:
              name: velocity-secrets
              key: api-key
        resources:
          requests:
            memory: "512Mi"
            cpu: "500m"
          limits:
            memory: "2Gi"
            cpu: "2000m"
        volumeMounts:
        - name: workspace
          mountPath: /home/velocity/workspace
      volumes:
      - name: workspace
        persistentVolumeClaim:
          claimName: velocity-workspace-pvc
---
apiVersion: v1
kind: Service
metadata:
  name: velocity-mcp
spec:
  selector:
    app: velocity-mcp
  ports:
  - port: 8080
    targetPort: 8080
  type: ClusterIP
```

---

## Configuration

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `VELOCITY_API_KEY` | API key for cloud providers | (required) |
| `RUST_LOG` | Log level (trace, debug, info, warn, error) | `info` |
| `VELOCITY_WORKSPACE` | Workspace directory | `./workspace` |
| `VELOCITY_CONFIG` | Config file path | `~/.velocity/config.toml` |

### Config File

Location: `~/.velocity/config.toml`

```toml
[general]
workspace = "/path/to/workspace"
log_level = "info"

[providers.openai]
api_key = "sk-..."
base_url = "https://api.openai.com/v1"

[providers.anthropic]
api_key = "sk-ant-..."

[providers.cloudflare]
api_key = "..."
account_id = "..."
```

---

## Monitoring

### Logs

Logs are written to stdout/stderr. Capture with systemd:

```bash
journalctl -u velocity-mcp -f
```

### Health Checks

The MCP server responds to JSON-RPC health requests:

```bash
echo '{"jsonrpc":"2.0","method":"health","id":1}' | \
  ./velocity_mcp --mode stdio
```

Expected response:
```json
{"jsonrpc":"2.0","result":{"status":"ok"},"id":1}
```

### Metrics

Metrics are exported via shared memory telemetry. Use the monitoring dashboard to view:
- Request counts
- Token usage
- Provider failover events
- Error rates

### Alerting

Set up alerts for:
- High error rates (>5%)
- Provider failover events
- Memory usage >80%
- Disk usage >90%

---

## Troubleshooting

### Build Failures

**Problem:** Missing GTK libraries (Linux)
```bash
sudo apt-get install libgtk-3-dev libwebkit2gtk-4.1-dev
```

**Problem:** Rust toolchain outdated
```bash
rustup update stable
```

### Runtime Issues

**Problem:** GPU acceleration not working
- Verify Vulkan runtime is installed
- Check `vulkaninfo` output
- Fall back to CPU mode (automatic)

**Problem:** API key not recognized
- Verify `VELOCITY_API_KEY` is set
- Check config file syntax
- Restart service after config changes

### Performance

**Problem:** Slow compilation
- Use `cargo build` (debug) for development
- Reserve `cargo build --release` for production

**Problem:** High memory usage
- Adjust `RUST_LOG` to reduce log volume
- Monitor with `htop` or `top`
- Consider scaling horizontally

---

## Security Best Practices

1. **Never commit API keys** — use environment variables or secret managers
2. **Run as non-root** — create dedicated `velocity` user
3. **Enable firewall** — restrict access to MCP server ports
4. **Regular updates** — keep Rust toolchain and dependencies current
5. **Audit logs** — monitor for suspicious activity
6. **Backup workspace** — regular backups of `.velocity/` directory

---

## Support

- **Documentation:** [README.md](../README.md)
- **Issues:** [GitHub Issues](https://github.com/UnitBuilds/Velocity-IDE/issues)
- **Discussions:** [GitHub Discussions](https://github.com/UnitBuilds/Velocity-IDE/discussions)
