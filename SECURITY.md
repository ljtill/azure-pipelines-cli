# Security Policy

## Reporting a Vulnerability

If you find a security issue in `azure-devops-cli`, please report it privately instead of opening a public issue.

1. Open a GitHub security advisory for this repository if that workflow is available to you.
2. If not, contact the maintainer directly through the repository owner contact path on GitHub and include:
   - a clear description of the issue
   - affected versions or commit range
   - reproduction steps or proof of concept
   - any suggested mitigation

Please avoid publishing exploit details until a fix or mitigation is available.

## Supported Versions

Security fixes are provided on the latest released version.

## Supply Chain Verification

Releases of `azure-devops-cli` are signed using [Sigstore](https://www.sigstore.dev/)
keyless signing via GitHub Actions OIDC (cosign). Every release publishes:

- `SHA256SUMS` — SHA-256 hashes of all release archives.
- `SHA256SUMS.cosign.bundle` — cosign bundle containing the Fulcio-issued
  signing certificate, signature, and Rekor transparency log entry.

### Expected signer identity

- **Certificate identity** (regex): `^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/tags/v.+$`
- **OIDC issuer**: `https://token.actions.githubusercontent.com`

### Manual verification

With [cosign](https://docs.sigstore.dev/cosign/installation/) installed:

```sh
TAG=v0.1.0   # or whichever release
BASE="https://github.com/ljtill/azure-devops-cli/releases/download/$TAG"
curl -fsSLO "$BASE/SHA256SUMS"
curl -fsSLO "$BASE/SHA256SUMS.cosign.bundle"

cosign verify-blob \
  --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/tags/v.+$' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  SHA256SUMS
```

### Automatic verification

Both install scripts (`install.sh`, `install.ps1`) and the built-in self-updater
(`devops update`) verify the cosign bundle automatically before installing any
archive. Verification **fails closed**: if `cosign` is missing or the signature
does not match the expected identity/issuer, installation is aborted. There is
no opt-out flag.
