package browser

import (
	"encoding/json"
	"fmt"
	"log"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
)

func serveExtension(extensionID string, extensionPath string) {
	// Start a simple HTTP server to serve the update manifest and CRX
	http.HandleFunc("/update_manifest.xml", func(w http.ResponseWriter, r *http.Request) {
		xml := fmt.Sprintf(`<?xml version='1.0' encoding='UTF-8'?>
<gupdate xmlns='http://www.google.com/update2/response' protocol='2.0'>
  <app appid='%s'>
    <updatecheck codebase='http://127.0.0.1:9999/src.crx' version='1.0' />
  </app>
</gupdate>`, extensionID)
		w.Header().Set("Content-Type", "application/xml")
		w.Write([]byte(xml))
	})

	http.HandleFunc("/src.crx", func(w http.ResponseWriter, r *http.Request) {
		crxPath := filepath.Join(extensionPath, "src.crx")
		http.ServeFile(w, r, crxPath)
	})

	go func() {
		log.Println("[Policy] Serving extension for Chrome policy at http://127.0.0.1:9999")
		http.ListenAndServe("127.0.0.1:9999", nil)
	}()
}

func EnforceExtensionPolicy(extensionID string, extensionPath string, proxyServer string) error {
	serveExtension(extensionID, extensionPath)
	updateURL := "http://127.0.0.1:9999/update_manifest.xml"
	policyStr := fmt.Sprintf("%s;%s", extensionID, updateURL)

	switch runtime.GOOS {
	case "windows":
		// Extension Policy
		key := `HKCU\SOFTWARE\Policies\Google\Chrome\ExtensionInstallForcelist`
		exec.Command("reg", "add", key, "/v", "1", "/t", "REG_SZ", "/d", policyStr, "/f").Run()

		// Proxy Policy (If provided)
		if proxyServer != "" {
			proxyKey := `HKCU\SOFTWARE\Policies\Google\Chrome`
			exec.Command("reg", "add", proxyKey, "/v", "ProxyMode", "/t", "REG_SZ", "/d", "fixed_servers", "/f").Run()
			exec.Command("reg", "add", proxyKey, "/v", "ProxyServer", "/t", "REG_SZ", "/d", proxyServer, "/f").Run()
		}

		// Whitelist local source
		whitelistKey := `HKCU\SOFTWARE\Policies\Google\Chrome\ExtensionInstallSources`
		exec.Command("reg", "add", whitelistKey, "/v", "1", "/t", "REG_SZ", "/d", "http://127.0.0.1/*", "/f").Run()

		// Native Messaging Host Registration
		hostKey := `HKCU\Software\Google\Chrome\NativeMessagingHosts\com.ghost.bridge.relay`
		hostManifestPath := filepath.Join(extensionPath, "com.ghost.bridge.relay.json")
		hostManifest := `{
  "name": "com.ghost.bridge.relay",
  "description": "Ghost Bridge Native Host",
  "path": "` + strings.ReplaceAll(filepath.Join(extensionPath, "..", "..", "ghost_bridge_host.exe"), `\`, `\\`) + `",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://` + extensionID + `/"
  ]
}`
		os.WriteFile(hostManifestPath, []byte(hostManifest), 0644)
		exec.Command("reg", "add", hostKey, "/ve", "/t", "REG_SZ", "/d", hostManifestPath, "/f").Run()

	case "linux":
		policyDir := "/etc/opt/chrome/policies/managed"
		os.MkdirAll(policyDir, 0755)

		policyMap := map[string]interface{}{
			"ExtensionInstallForcelist": []string{policyStr},
			"ExtensionInstallSources":   []string{"http://127.0.0.1/*"},
		}

		if proxyServer != "" {
			policyMap["ProxyMode"] = "fixed_servers"
			policyMap["ProxyServer"] = proxyServer
		}

		policyData, _ := json.MarshalIndent(policyMap, "", "  ")

		policyPath := filepath.Join(policyDir, "ghost_bridge.json")
		if err := os.WriteFile(policyPath, policyData, 0644); err != nil {
			log.Printf("[Policy] Failed to write linux policy (requires root): %v", err)
		} else {
			log.Printf("[Policy] Linux policy written to %s", policyPath)
		}

		// Also write NativeMessagingHost manifest for Linux
		homeDir, _ := os.UserHomeDir()
		hostPaths := []string{
			filepath.Join(homeDir, ".config/google-chrome/NativeMessagingHosts"),
			filepath.Join(homeDir, ".config/chromium/NativeMessagingHosts"),
			"/etc/opt/chrome/native-messaging-hosts",
			"/usr/lib/google-chrome/native-messaging-hosts",
			"/app/ghost_chrome_profile/NativeMessagingHosts",
		}
		
		hostManifest := `{
  "name": "com.ghost.bridge.relay",
  "description": "Ghost Bridge Native Host",
  "path": "/app/ghost_bridge_host",
  "type": "stdio",
  "allowed_origins": [
    "chrome-extension://` + extensionID + `/"
  ]
}`
		for _, hp := range hostPaths {
			os.MkdirAll(hp, 0755)
			manifestPath := filepath.Join(hp, "com.ghost.bridge.relay.json")
			if err := os.WriteFile(manifestPath, []byte(hostManifest), 0644); err != nil {
				log.Printf("[Policy] Failed to write manifest to %s: %v", manifestPath, err)
			} else {
				log.Printf("[Policy] Native messaging manifest written to %s", manifestPath)
			}
		}
		log.Printf("[Policy] Manifest content:\n%s", hostManifest)
	}

	return nil
}
