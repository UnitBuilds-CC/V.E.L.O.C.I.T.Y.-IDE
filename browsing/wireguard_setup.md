# Wireguard Autonomous Swarm Setup

The Wireguard configurations have been generated with unique cryptographic keys for each node.

## Node Map
- **Dev PC (Rack)**: `10.0.0.1`
    - **Roles**: Orchestrator, LM Studio (4b Model), Neo4j.
    - **Wireguard Config**: `wireguard/configs/Rack.conf`
- **Laptop (Local PC)**: `10.0.0.2`
    - **Roles**: Node Agent (Browser Automation).
    - **Wireguard Config**: `wireguard/configs/LocalPC.conf`
- **Tablet (Mobile)**: `10.0.0.5`
    - **Roles**: Mission Initiator (Sovereign Link).
    - **Wireguard Config**: `wireguard/configs/Mobile.conf`

## 1. Deploying Configs

### [Rack] Server Setup
1. Copy `wireguard/configs/Rack.conf` to `/etc/wireguard/wg0.conf`.
2. Ensure port `51820/UDP` is open on your firewall.
3. Start Wireguard: `sudo wg-quick up wg0`.

### [Local PC] Windows Setup
1. Open the Wireguard for Windows app.
2. Click "Add Tunnel" -> "Import tunnel from file...".
3. Select `c:\go-engine\wireguard\configs\LocalPC.conf`.
4. **IMPORTANT**: Edit the config and replace `[RACK_PUBLIC_IP]` with the actual public IP of your Rack server.
5. Click "Activate".

### [Mobile] Android/iOS Setup
1. Install the Wireguard app from the App Store or Play Store.
2. Create a new tunnel from the `Mobile.conf` file.
3. **IMPORTANT**: Replace `[RACK_PUBLIC_IP]` with your Rack's public IP.
4. Toggle the switch to connect.

## 2. Verification
Once all nodes are active, you should be able to ping the Rack from any node:
```bash
ping 10.0.0.1
```

The Swarm Orchestrator should be configured to listen on `10.0.0.1:50052` to accept missions from the mobile device.

### Triggering a Mission (Mobile)
Run the `mobile_link` tool from your mobile device (e.g., via Termux):
```bash
export ORCHESTRATOR_ADDR="10.0.0.1:50052"
./mobile_link "https://amazon.com" "Find the cheapest laptop"
```

## Security Notes
- All gRPC traffic flows through the encrypted Wireguard tunnel.
- The Orchestrator only accepts `SubmitMission` from authorized initiator IPs.
- Secrets never leave the Local PC/Vault except when explicitly authorized for a specific mission.

## 1. Concept

- **Tier 1 (Server Rack)**: The primary high-availability host for LM Studio (e.g., `10.0.0.1`).
- **Tier 2 (Failover PC)**: A secondary host running LM Studio in case the rack goes down (e.g., `10.0.0.2`).
- **Orchestrator**: Connects to both and fails over automatically.
- **Network**: All devices are in the `10.0.0.0/24` subnet.

## 2. Configuration Template

We have created a template at `wireguard/client.conf.template`. To support multi-tiering, you should define **both** peers in your client config:

```ini
[Interface]
PrivateKey = <CLIENT_PRIVATE_KEY>
Address = 10.0.0.3/24 # IP of this client (Orchestrator)

# Tier 1: Server Rack
[Peer]
PublicKey = <SERVER_RACK_PUBLIC_KEY>
Endpoint = <SERVER_RACK_IP>:51820
AllowedIPs = 10.0.0.1/32
PersistentKeepalive = 25

# Tier 2: Failover PC
[Peer]
PublicKey = <PC_PUBLIC_KEY>
Endpoint = <PC_IP>:51820
AllowedIPs = 10.0.0.2/32
PersistentKeepalive = 25
```

## 3. Deployment Options

### Option A: Host-Level (Windows)
Since the Orchestrator is running on Windows:
1.  Install the official **WireGuard for Windows** app.
2.  Import a new tunnel using the filled-out template.
3.  Activate the tunnel.
4.  Set the environment variables for failover:
    ```powershell
    $env:LM_STUDIO_URL="http://10.0.0.1:1234/v1" # Server Rack
    $env:LM_STUDIO_URL_FALLBACK="http://10.0.0.2:1234/v1" # Failover PC
    ```

### Option B: Container-Level (Podman/Docker)
If you prefer to run everything in a pod:
1.  Add a Wireguard sidecar container to the pod.
2.  Use a volume to mount the `client.conf`.
3.  Set the same environment variables in the Orchestrator container.

## 4. Verification

Once connected, verify you can reach LM Studio on both endpoints:
```powershell
curl http://10.0.0.1:1234/v1/models
curl http://10.0.0.2:1234/v1/models
```
The Orchestrator will automatically try the Server Rack first and fall back to the PC if it fails!
