package vault

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"encoding/json"
	"fmt"
	"io"
	"os"

	"golang.org/x/crypto/argon2"
)

// Vault holds sensitive user data in memory.
type Vault struct {
	Secrets map[string]map[string]string `json:"secrets"` // Site -> Key -> Value
	path    string
}

// EncryptedData is the structure saved to disk.
type EncryptedData struct {
	Salt       []byte `json:"salt"`
	Nonce      []byte `json:"nonce"`
	Ciphertext []byte `json:"ciphertext"`
}

// NewVault creates a new Vault instance.
func NewVault(path string) *Vault {
	return &Vault{
		Secrets: make(map[string]map[string]string),
		path:    path,
	}
}

// AddSecret adds or updates a secret in the vault.
func (v *Vault) AddSecret(site, key, value string) {
	if v.Secrets[site] == nil {
		v.Secrets[site] = make(map[string]string)
	}
	v.Secrets[site][key] = value
}

// GetSecret retrieves a secret from the vault.
func (v *Vault) GetSecret(site, key string) (string, error) {
	if v.Secrets[site] == nil {
		return "", fmt.Errorf("no secrets found for site %s", site)
	}
	val, ok := v.Secrets[site][key]
	if !ok {
		return "", fmt.Errorf("no secret found for key %s at site %s", key, site)
	}
	return val, nil
}

// Save encrypts and saves the vault to disk.
func (v *Vault) Save(masterPassword string) error {
	plaintext, err := json.Marshal(v.Secrets)
	if err != nil {
		return err
	}

	salt := make([]byte, 16)
	if _, err := io.ReadFull(rand.Reader, salt); err != nil {
		return err
	}

	key := argon2.IDKey([]byte(masterPassword), salt, 1, 64*1024, 4, 32)

	block, err := aes.NewCipher(key)
	if err != nil {
		return err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return err
	}

	nonce := make([]byte, gcm.NonceSize())
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return err
	}

	ciphertext := gcm.Seal(nil, nonce, plaintext, nil)

	data := EncryptedData{
		Salt:       salt,
		Nonce:      nonce,
		Ciphertext: ciphertext,
	}

	fileData, err := json.Marshal(data)
	if err != nil {
		return err
	}

	return os.WriteFile(v.path, fileData, 0600)
}

// Load decrypts and loads the vault from disk.
func (v *Vault) Load(masterPassword string) error {
	fileData, err := os.ReadFile(v.path)
	if err != nil {
		return err
	}

	var data EncryptedData
	if err := json.Unmarshal(fileData, &data); err != nil {
		return err
	}

	key := argon2.IDKey([]byte(masterPassword), data.Salt, 1, 64*1024, 4, 32)

	block, err := aes.NewCipher(key)
	if err != nil {
		return err
	}

	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return err
	}

	plaintext, err := gcm.Open(nil, data.Nonce, data.Ciphertext, nil)
	if err != nil {
		return fmt.Errorf("decryption failed: %v", err)
	}

	return json.Unmarshal(plaintext, &v.Secrets)
}
