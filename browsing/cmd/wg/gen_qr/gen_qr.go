package main

import (
	"fmt"
	"os"
	"strings"

	qrcode "github.com/skip2/go-qrcode"
)

func main() {
	configData, err := os.ReadFile("wireguard/configs/Mobile.conf")
	if err != nil {
		fmt.Printf("Error reading Mobile.conf: %v\n", err)
		return
	}

	content := strings.ReplaceAll(string(configData), "\r\n", "\n")
	fmt.Printf("Encoding string (normalized):\n---\n%s\n---\n", content)
	err = qrcode.WriteFile(content, qrcode.Medium, 256, "wireguard/configs/Mobile_QR.png")
	if err != nil {
		fmt.Printf("Error generating QR code: %v\n", err)
		return
	}

	fmt.Println("QR Code generated: wireguard/configs/Mobile_QR.png")
}
