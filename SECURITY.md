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

## CI Security Scanning

GitHub Actions runs `cargo audit` and `cargo deny --all-features check` on
pushes, pull requests, and a weekly schedule. Advisory ignores are allowed only
when `.cargo/audit.toml` and `deny.toml` both document the advisory link,
rationale, Reviewed date, Review-by date, and early re-review trigger.

## Supply Chain Verification

Releases of `azure-devops-cli` are signed using [Sigstore](https://www.sigstore.dev/)
keyless signing via GitHub Actions OIDC (cosign). Every release publishes:

- `SHA256SUMS` — SHA-256 hashes of all release archives.
- `SHA256SUMS.cosign.bundle` — cosign bundle containing the Fulcio-issued
  signing certificate, signature, and Rekor transparency log entry.

### Expected signer identity

- **Certificate identity** (regex): `^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/heads/main$`
- **OIDC issuer**: `https://token.actions.githubusercontent.com`

### Manual verification

Use a trusted `cosign` binary installed through an approved channel (or invoke it
by absolute path). Then verify the signed checksum manifest before trusting any
archive hash:

```sh
TAG=v0.1.0   # or whichever release
BASE="https://github.com/ljtill/azure-devops-cli/releases/download/$TAG"
curl -fsSLO "$BASE/SHA256SUMS"
curl -fsSLO "$BASE/SHA256SUMS.cosign.bundle"

cosign verify-blob \
  --bundle SHA256SUMS.cosign.bundle \
  --certificate-identity-regexp '^https://github\.com/ljtill/azure-devops-cli/\.github/workflows/ci\.release\.yml@refs/heads/main$' \
  --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
  SHA256SUMS
```

After the bundle verifies, download the platform archive and compare its SHA-256
digest with the matching `SHA256SUMS` line before extracting it. Use approved
hashing and extraction tools, and avoid relying on a writable directory earlier
in `PATH` for `cosign`, `sha256sum`/`shasum`, `tar`, or similar tools.

### Automatic verification

The install scripts (`install.sh`, `install.ps1`) download `SHA256SUMS` and
`SHA256SUMS.cosign.bundle`, run `cosign verify-blob`, and compare the archive
SHA-256 before installing. Verification **fails closed**: if `cosign` is missing
or the signature/hash does not match the expected identity and issuer, the
installation is aborted. There is no opt-out flag.

The built-in self-updater (`devops update`) performs Sigstore bundle
verification in process and computes SHA-256 in process, so it does not invoke a
`cosign` or hashing binary from `PATH`. It streams downloads with size caps,
stages verified archives before promotion, extracts only the expected binary
member, and uses a two-phase update lock so startup can roll back an interrupted
swap.

The release workflow also verifies `SHA256SUMS`, every produced archive checksum,
and the cosign bundle with the same identity and issuer before publishing assets.
