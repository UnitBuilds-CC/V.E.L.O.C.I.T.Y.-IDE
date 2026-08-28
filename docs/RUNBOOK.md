# Operational Runbook

This runbook provides procedures for common operational tasks and incident response for Velocity IDE.

## Table of Contents

- [Quick Reference](#quick-reference)
- [Monitoring](#monitoring)
- [Common Procedures](#common-procedures)
- [Incident Response](#incident-response)
- [Maintenance](#maintenance)
- [Troubleshooting](#troubleshooting)

---

## Quick Reference

### Service Status

```bash
# Check service status
systemctl status velocity-mcp

# View recent logs
journalctl -u velocity-mcp -n 50 --no-pager

# Check health
echo '{"jsonrpc":"2.0","method":"health","id":1}' | velocity_mcp --mode stdio
```

### Emergency Contacts

- **Primary On-Call:** [Add contact]
- **Secondary On-Call:** [Add contact]
- **Escalation:** [Add contact]

### Critical Paths

- **Binary Location:** `/opt/velocity/velocity_mcp`
- **Config:** `/opt/velocity/config.toml`
- **Logs:** `journalctl -u velocity-mcp`
- **Workspace:** `/opt/velocity/workspace`
- **PID File:** `/var/run/velocity-mcp.pid`

---

## Monitoring

### Key Metrics

| Metric | Normal Range | Warning | Critical |
|--------|--------------|---------|----------|
| CPU Usage | < 50% | 50-80% | > 80% |
| Memory Usage | < 60% | 60-85% | > 85% |
| Disk Usage | < 70% | 70-90% | > 90% |
| Error Rate | < 1% | 1-5% | > 5% |
| Response Time | < 500ms | 500ms-2s | > 2s |
| Provider Failovers | 0 | 1-2/hour | > 2/hour |

### Log Analysis

```bash
# Search for errors
journalctl -u velocity-mcp --since "1 hour ago" | grep -i error

# Count errors by type
journalctl -u velocity-mcp --since "1 hour ago" | \
  grep "ERROR" | awk '{print $5}' | sort | uniq -c | sort -rn

# Monitor in real-time
journalctl -u velocity-mcp -f
```

### Health Checks

```bash
# Basic health check
curl -s http://localhost:8080/health || echo "UNHEALTHY"

# Detailed health (if implemented)
curl -s http://localhost:8080/health/detailed | jq .

# MCP protocol health
echo '{"jsonrpc":"2.0","method":"health","id":1}' | \
  timeout 5 velocity_mcp --mode stdio
```

---

## Common Procedures

### Deploy New Version

```bash
# 1. Stop service
sudo systemctl stop velocity-mcp

# 2. Backup current binary
sudo cp /opt/velocity/velocity_mcp /opt/velocity/velocity_mcp.bak.$(date +%Y%m%d-%H%M%S)

# 3. Deploy new binary
sudo cp target/release/velocity_mcp /opt/velocity/

# 4. Verify permissions
sudo chmod +x /opt/velocity/velocity_mcp
sudo chown velocity:velocity /opt/velocity/velocity_mcp

# 5. Start service
sudo systemctl start velocity-mcp

# 6. Verify health
sleep 5
systemctl status velocity-mcp
echo '{"jsonrpc":"2.0","method":"health","id":1}' | velocity_mcp --mode stdio

# 7. Monitor logs for 5 minutes
journalctl -u velocity-mcp -f
```

### Rollback Deployment

```bash
# 1. Identify last good version
ls -la /opt/velocity/velocity_mcp.bak.*

# 2. Stop service
sudo systemctl stop velocity-mcp

# 3. Restore backup
sudo mv /opt/velocity/velocity_mcp.bak.YYYYMMDD-HHMMSS /opt/velocity/velocity_mcp

# 4. Start service
sudo systemctl start velocity-mcp

# 5. Verify
systemctl status velocity-mcp
```

### Rotate API Keys

```bash
# 1. Update config file
sudo vim /opt/velocity/config.toml

# 2. Restart service to pick up new keys
sudo systemctl restart velocity-mcp

# 3. Verify new keys work
echo '{"jsonrpc":"2.0","method":"providers_list","id":1}' | \
  velocity_mcp --mode stdio

# 4. Monitor for authentication errors
journalctl -u velocity-mcp -f | grep -i "auth\|401\|403"
```

### Scale Horizontally

```bash
# 1. Update systemd service (if using multiple instances)
sudo systemctl edit velocity-mcp@.service

# 2. Start additional instances
sudo systemctl start velocity-mcp@1
sudo systemctl start velocity-mcp@2

# 3. Verify all instances
systemctl list-units 'velocity-mcp@*'

# 4. Update load balancer configuration
# (Add new instance endpoints)
```

### Clear Cache

```bash
# 1. Stop service
sudo systemctl stop velocity-mcp

# 2. Clear cache directories
sudo rm -rf /opt/velocity/.velocity/cache/*
sudo rm -rf /opt/velocity/.velocity/browser_artifacts/*

# 3. Restart service
sudo systemctl start velocity-mcp

# 4. Monitor rebuild
journalctl -u velocity-mcp -f
```

---

## Incident Response

### High Error Rate (>5%)

**Symptoms:**
- Error rate > 5% in logs
- User complaints about failures
- Alert triggered

**Diagnosis:**
```bash
# Check recent errors
journalctl -u velocity-mcp --since "15 minutes ago" | grep ERROR

# Identify error patterns
journalctl -u velocity-mcp --since "15 minutes ago" | \
  grep ERROR | awk '{print $5, $6}' | sort | uniq -c | sort -rn

# Check provider status
echo '{"jsonrpc":"2.0","method":"providers_status","id":1}' | \
  velocity_mcp --mode stdio
```

**Resolution:**
1. If provider errors: Check provider status pages, consider failover
2. If rate limiting: Reduce request rate or increase limits
3. If validation errors: Check recent config changes
4. If memory errors: Restart service and investigate memory leak

**Escalation:** If error rate > 10% for > 5 minutes, escalate to on-call engineer.

### Service Unresponsive

**Symptoms:**
- Health check fails
- No response to requests
- Service appears hung

**Diagnosis:**
```bash
# Check if process exists
ps aux | grep velocity_mcp

# Check resource usage
top -p $(pgrep velocity_mcp)

# Check for deadlocks (if Rust backtrace enabled)
RUST_BACKTRACE=1 journalctl -u velocity-mcp -n 100
```

**Resolution:**
```bash
# 1. Graceful restart
sudo systemctl restart velocity-mcp

# 2. If unresponsive, force kill
sudo systemctl kill -s SIGKILL velocity-mcp
sudo systemctl start velocity-mcp

# 3. Check for core dumps
ls -la /var/lib/systemd/coredump/

# 4. Analyze crash (if core dump exists)
gdb /opt/velocity/velocity_mcp /var/lib/systemd/coredump/core.*
```

**Escalation:** If service crashes 3+ times in 1 hour, escalate immediately.

### Provider Failover Storm

**Symptoms:**
- Multiple provider failovers in short time
- Degraded performance
- Increased latency

**Diagnosis:**
```bash
# Check failover events
journalctl -u velocity-mcp --since "1 hour ago" | grep -i "failover\|fallback"

# Check provider health
echo '{"jsonrpc":"2.0","method":"providers_health","id":1}' | \
  velocity_mcp --mode stdio
```

**Resolution:**
1. Identify failing provider from logs
2. Check provider status page
3. Temporarily disable failing provider in config
4. Monitor remaining providers for capacity
5. Contact provider support if needed

**Escalation:** If all providers fail, escalate to engineering lead.

### Disk Space Critical (>90%)

**Symptoms:**
- Disk usage alert
- Write failures in logs
- Service degradation

**Diagnosis:**
```bash
# Check disk usage
df -h /opt/velocity

# Find large files
sudo du -sh /opt/velocity/* | sort -hr | head -20

# Check log size
sudo du -sh /var/log/journal/*
```

**Resolution:**
```bash
# 1. Clear old logs
sudo journalctl --vacuum-time=7d

# 2. Clear cache
sudo rm -rf /opt/velocity/.velocity/cache/*

# 3. Archive old workspace data
sudo find /opt/velocity/workspace -name "*.log" -mtime +30 -delete

# 4. If still critical, expand disk
# (Cloud-specific procedure)
```

**Escalation:** If disk > 95% and cannot free space, escalate immediately.

### Memory Leak Suspected

**Symptoms:**
- Memory usage steadily increasing
- OOM killer invoked
- Service crashes with OOM

**Diagnosis:**
```bash
# Monitor memory over time
watch -n 5 'ps -o pid,rss,vsz,comm -p $(pgrep velocity_mcp)'

# Check for OOM events
dmesg | grep -i "oom\|killed"

# Generate heap profile (if compiled with profiling)
MALLOC_STATS_=1 velocity_mcp --mode stdio 2>&1 | grep -A 20 "Heap"
```

**Resolution:**
1. Restart service to free memory
2. Enable memory profiling in debug build
3. Analyze heap dumps
4. Identify and fix memory leak in code

**Escalation:** If memory grows > 1GB/hour, escalate to engineering.

---

## Maintenance

### Weekly Tasks

- [ ] Review error logs for patterns
- [ ] Check disk usage trends
- [ ] Verify backup completion
- [ ] Review provider usage metrics
- [ ] Update threat model if needed

### Monthly Tasks

- [ ] Update Rust toolchain: `rustup update`
- [ ] Update dependencies: `cargo update`
- [ ] Review and rotate API keys
- [ ] Audit user access logs
- [ ] Test disaster recovery procedure
- [ ] Review and update runbook

### Quarterly Tasks

- [ ] Security audit (automated + manual)
- [ ] Performance benchmarking
- [ ] Capacity planning review
- [ ] Disaster recovery drill
- [ ] Update incident response contacts
- [ ] Review and optimize costs

---

## Troubleshooting

### Service Won't Start

**Check:**
```bash
# Systemd logs
journalctl -u velocity-mcp -n 50

# Binary exists and is executable
ls -la /opt/velocity/velocity_mcp

# Config file syntax
velocity_mcp --check

# Port availability
ss -tlnp | grep 8080
```

**Common Causes:**
- Missing dependencies (libgtk, libwebkit)
- Config file syntax error
- Port already in use
- Insufficient permissions

### Slow Performance

**Check:**
```bash
# CPU usage
top -p $(pgrep velocity_mcp)

# I/O wait
iostat -x 1 5

# Network latency
ping <provider-endpoint>

# Database/query performance
# (If applicable)
```

**Common Causes:**
- Resource exhaustion (CPU, memory, disk I/O)
- Network latency to providers
- Large workspace size
- Inefficient queries

### Configuration Issues

**Validate config:**
```bash
# Check syntax
velocity_mcp --check

# Test provider connectivity
velocity_mcp --providers test

# View effective config
velocity_mcp --config show
```

**Common Issues:**
- Invalid TOML syntax
- Missing required fields
- Incorrect file permissions
- Environment variable overrides

---

## Appendix

### Useful Commands

```bash
# Quick health check
echo '{"jsonrpc":"2.0","method":"health","id":1}' | velocity_mcp --mode stdio | jq .

# List all providers
echo '{"jsonrpc":"2.0","method":"providers_list","id":1}' | velocity_mcp --mode stdio | jq .

# Get usage statistics
echo '{"jsonrpc":"2.0","method":"usage_stats","id":1}' | velocity_mcp --mode stdio | jq .

# Reload configuration
sudo systemctl reload velocity-mcp

# View service dependencies
systemctl list-dependencies velocity-mcp

# Check service logs since last boot
journalctl -u velocity-mcp -b
```

### Debug Mode

Enable debug logging:
```bash
# Temporary (until restart)
sudo systemctl set-environment velocity-mcp RUST_LOG=debug

# Permanent
sudo systemctl edit velocity-mcp
# Add: Environment="RUST_LOG=debug"

# Restart to apply
sudo systemctl restart velocity-mcp
```

### Contact Information

- **Documentation:** [DEPLOYMENT.md](DEPLOYMENT.md)
- **Architecture:** [README.md](../README.md)
- **GitHub:** https://github.com/UnitBuilds/Velocity-IDE
- **Support:** support@velocity-ide.com
