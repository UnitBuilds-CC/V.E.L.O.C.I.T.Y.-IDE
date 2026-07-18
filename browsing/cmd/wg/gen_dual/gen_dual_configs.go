package main

import (
	"fmt"
	"os"
	"strings"

	qrcode "github.com/skip2/go-qrcode"
)

func main() {
	localIP := "192.168.1.162"
	publicIP := "197.234.87.122"
	port := "51820"

	// Laptop Keys
	pcPriv := "cKY/aWmneJRDhADBvEn2fcFiIJvdiKg5OnC5mQZRFkU="
	
	// Tablet Keys
	mobilePriv := "8NH9YVP9L0HaQqdVK2MbchL0t1VhLFkysR0Vn9Qv+G4="

	// Rack Public Key
	rackPub := "HvvZCwpe4akfd6pUplsowTpk2O/CF8kxUxT0aF5c62U="

	os.MkdirAll("wireguard/configs", 0755)

	configs := []struct {
		name     string
		priv     string
		addr     string
		endpoint string
	}{
		{"Laptop_Home", pcPriv, "10.0.0.2/24", localIP + ":" + port},
		{"Laptop_Remote", pcPriv, "10.0.0.2/24", publicIP + ":" + port},
		{"Tablet_Home", mobilePriv, "10.0.0.5/24", localIP + ":" + port},
		{"Tablet_Remote", mobilePriv, "10.0.0.5/24", publicIP + ":" + port},
	}

	for _, c := range configs {
		content := fmt.Sprintf(`[Interface]
PrivateKey = %s
Address = %s

[Peer]
PublicKey = %s
Endpoint = %s
AllowedIPs = 10.0.0.0/24
PersistentKeepalive = 25
`, c.priv, c.addr, rackPub, c.endpoint)

		// Force Unix line endings
		content = strings.ReplaceAll(content, "\r\n", "\n")
		
		path := fmt.Sprintf("wireguard/configs/%s.conf", c.name)
		os.WriteFile(path, []byte(content), 0644)
		fmt.Printf("Generated %s\n", path)

		// Generate QR for Tablet versions
		if strings.HasPrefix(c.name, "Tablet") {
			qrPath := fmt.Sprintf("wireguard/configs/%s_QR.png", c.name)
			qrcode.WriteFile(content, qrcode.Medium, 256, qrPath)
			fmt.Printf("Generated QR: %s\n", qrPath)
		}
	}
}
