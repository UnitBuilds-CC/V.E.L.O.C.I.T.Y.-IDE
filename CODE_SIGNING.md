# Windows Code Signing Guide

## Prerequisites

1. **Code Signing Certificate**
   - Purchase from a trusted CA (DigiCert, Sectigo, GlobalSign)
   - For EV certificates: requires hardware token (USB)
   - For standard certificates: PFX/P12 file with private key

2. **Windows SDK**
   - Install Windows SDK 10.0.22621.0 or later
   - Signtool.exe is located at: `C:\Program Files (x86)\Windows Kits\10\bin\<version>\x64\signtool.exe`

3. **Inno Setup**
   - Download from: https://jrsoftware.org/isinfo.php
   - ISCC.exe is the command-line compiler

## Building a Signed Installer

### Standard Certificate (PFX file)

```powershell
$certPath = "C:\path\to\certificate.pfx"
$certPassword = "your-password"
$signtool = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe"

# Build installer with signing
& "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" `
    /DMyAppVersion=1.0.0 `
    /S"signtool=$signtool sign /f `"$certPath`" /p `"$certPassword`" /tr http://timestamp.digicert.com /td sha256 /fd sha256 `$f" `
    installer.iss
```

### EV Certificate (Hardware Token)

For EV certificates with hardware tokens, use the token's signing tool:

```powershell
# Example for SafeNet/Thales token
$signtool = "C:\Program Files (x86)\Windows Kits\10\bin\10.0.22621.0\x64\signtool.exe"

& "C:\Program Files (x86)\Inno Setup 6\ISCC.exe" `
    /DMyAppVersion=1.0.0 `
    /S"signtool=$signtool sign /n `"Your Company Name`" /tr http://timestamp.digicert.com /td sha256 /fd sha256 `$f" `
    installer.iss
```

## Timestamping

Always use RFC 3161 timestamping (`/tr` flag) instead of Authenticode (`/t`):
- **DigiCert**: `http://timestamp.digicert.com`
- **Sectigo**: `http://timestamp.sectigo.com`
- **GlobalSign**: `http://timestamp.globalsign.com`

## Verifying Signatures

```powershell
# Check installer signature
Get-AuthenticodeSignature .\output\VELOCITY-1.0.0-Setup.exe

# Expected output:
# Status    : Valid
# SignerCertificate : [certificate details]
```

## CI/CD Integration

For GitHub Actions or similar CI:

```yaml
- name: Build Signed Installer
  env:
    SIGN_CERT_BASE64: ${{ secrets.SIGN_CERT_BASE64 }}
    SIGN_CERT_PASSWORD: ${{ secrets.SIGN_CERT_PASSWORD }}
  run: |
    # Decode certificate from base64 secret
    $certBytes = [Convert]::FromBase64String($env:SIGN_CERT_BASE64)
    [IO.File]::WriteAllBytes("$env:TEMP\cert.pfx", $certBytes)
    
    # Build with signing
    & ".\build_signed_installer.ps1" -CertPath "$env:TEMP\cert.pfx" -CertPassword $env:SIGN_CERT_PASSWORD
```

## Troubleshooting

### "The file is recognized by the system but is not yet processed"
- Wait 5-10 minutes after signing for SmartScreen to recognize the new certificate
- EV certificates get immediate SmartScreen reputation

### "The signature is expired"
- Renew your code signing certificate
- Re-sign the installer with the new certificate

### "Untrusted publisher" warning
- Standard certificates need time to build reputation
- Consider an EV certificate for immediate trust

## Security Best Practices

1. **Never commit certificates** to source control
2. **Use CI secrets** for certificate storage
3. **Rotate certificates** before expiration
4. **Use strong passwords** for PFX files
5. **Enable hardware token** for EV certificates
6. **Timestamp all signatures** to ensure validity after certificate expiration
