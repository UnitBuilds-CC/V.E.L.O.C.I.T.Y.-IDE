package main

import (
	"fmt"
	"os"
	"os/exec"
	"strings"
)

func genKey() (string, string) {
	priv, _ := exec.Command("wg", "genkey").Output()
	privStr := strings.TrimSpace(string(priv))
	
	cmd := exec.Command("wg", "pubkey")
	cmd.Stdin = strings.NewReader(privStr)
	pub, _ := cmd.Output()
	pubStr := strings.TrimSpace(string(pub))
	
	return privStr, pubStr
}

func main() {
	rackPriv, rackPub := genKey()
	pcPriv, pcPub := genKey()
	mobilePriv, mobilePub := genKey()

	fmt.Printf("Rack: %s / %s\n", rackPriv, rackPub)
	fmt.Printf("PC: %s / %s\n", pcPriv, pcPub)
	fmt.Printf("Mobile: %s / %s\n", mobilePriv, mobilePub)

	// Update Rack.conf
	rackConf := fmt.Sprintf(`[Interface]
PrivateKey = %s
Address = 10.0.0.1/24
ListenPort = 51820

# Peer: LocalPC
[Peer]
PublicKey = %s
AllowedIPs = 10.0.0.2/32
PersistentKeepalive = 25

# Peer: Mobile
[Peer]
PublicKey = %s
AllowedIPs = 10.0.0.5/32
PersistentKeepalive = 25
`, rackPriv, pcPub, mobilePub)

	// Update LocalPC.conf
	pcConf := fmt.Sprintf(`[Interface]
PrivateKey = %s
Address = 10.0.0.2/24

# Peer: Rack (Server)
[Peer]
PublicKey = %s
Endpoint = [RACK_PUBLIC_IP]:51820
AllowedIPs = 10.0.0.0/24
PersistentKeepalive = 25
`, pcPriv, rackPub)

	// Update Mobile.conf
	mobileConf := fmt.Sprintf(`[Interface]
PrivateKey = %s
Address = 10.0.0.5/24

# Peer: Rack (Server)
[Peer]
PublicKey = %s
Endpoint = [RACK_PUBLIC_IP]:51820
AllowedIPs = 10.0.0.0/24
PersistentKeepalive = 25
`, mobilePriv, rackPub)

	os.MkdirAll("wireguard/configs", 0755)
	os.WriteFile("wireguard/configs/Rack.conf", []byte(rackConf), 0644)
	os.WriteFile("wireguard/configs/LocalPC.conf", []byte(pcConf), 0644)
	os.WriteFile("wireguard/configs/Mobile.conf", []byte(mobileConf), 0644)

	fmt.Println("Configs updated with real keys. Note: Replace [RACK_PUBLIC_IP] with your actual Rack IP.")
}
