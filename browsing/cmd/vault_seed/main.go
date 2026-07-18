package main

import (
	"fmt"
	"os"

	"github.com/reclamation-admin/agentic-browser-go/pkg/vault"
)

func main() {
	masterPwd := os.Getenv("VAULT_PASSWORD")
	if masterPwd == "" {
		fmt.Println("Error: VAULT_PASSWORD not set")
		return
	}

	os.MkdirAll("vault", 0755)
	v := vault.NewVault("vault/secrets.enc")
	
	v.AddSecret("google.com", "username", "testuser@gmail.com")
	v.AddSecret("google.com", "password", "super_secure_password_123")
	v.AddSecret("bank.com", "pin", "1234")

	if err := v.Save(masterPwd); err != nil {
		fmt.Printf("Error saving vault: %v\n", err)
		return
	}

	fmt.Println("Vault seeded successfully at vault/secrets.enc")
}
