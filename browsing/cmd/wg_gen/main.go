package main

import (
	"fmt"
	"os"
	"text/template"
)

type Peer struct {
	Name       string
	IP         string
	PublicKey  string
	PrivateKey string
	Endpoint   string
}

const configTemplate = `
# Wireguard Config for {{.Name}}
[Interface]
PrivateKey = {{.PrivateKey}}
Address = {{.IP}}/24
ListenPort = 51820

{{range .Peers}}
# Peer: {{.Name}}
[Peer]
PublicKey = {{.PublicKey}}
AllowedIPs = {{.IP}}/32
{{if .Endpoint}}Endpoint = {{.Endpoint}}:51820{{end}}
PersistentKeepalive = 25
{{end}}
`

func main() {
	// In a real scenario, we'd generate keys using 'wg genkey'
	// For this tool, we'll assume keys are provided or use placeholders
	
	rack := Peer{Name: "Rack", IP: "10.0.0.1", PublicKey: "RACK_PUB_KEY", PrivateKey: "RACK_PRIV_KEY", Endpoint: "RACK_PUBLIC_IP"}
	pc := Peer{Name: "LocalPC", IP: "10.0.0.2", PublicKey: "PC_PUB_KEY", PrivateKey: "PC_PRIV_KEY"}
	mobile := Peer{Name: "Mobile", IP: "10.0.0.5", PublicKey: "MOBILE_PUB_KEY", PrivateKey: "MOBILE_PRIV_KEY"}

	peers := []Peer{rack, pc, mobile}

	os.MkdirAll("wireguard/configs", 0755)

	tmpl := template.Must(template.New("wg").Parse(configTemplate))

	for _, p := range peers {
		filename := fmt.Sprintf("wireguard/configs/%s.conf", p.Name)
		f, _ := os.Create(filename)
		
		// Filter other peers
		otherPeers := []Peer{}
		for _, op := range peers {
			if op.Name != p.Name {
				otherPeers = append(otherPeers, op)
			}
		}

		tmpl.Execute(f, struct {
			Name       string
			IP         string
			PrivateKey string
			Peers      []Peer
		}{
			Name:       p.Name,
			IP:         p.IP,
			PrivateKey: p.PrivateKey,
			Peers:      otherPeers,
		})
		f.Close()
		fmt.Printf("Generated %s\n", filename)
	}
}
