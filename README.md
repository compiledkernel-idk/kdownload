# kdownload

`kdownload` is a blazing-fast, async command-line downloader for Linux. It uses segmented range requests, adaptive scheduling, and mirror-aware balancing to squeeze as much bandwidth as possible out of every host.

## Highlights
- **Async Rust core** built on Tokio and Reqwest with rustls.
- **Segmented multi-connection downloads** with dynamic concurrency tuning.
- **Robust resume support** via `.kdl.partmap` sidecar files and range validation.
- **Mirror awareness** to balance segments across multiple URLs.
- **Bandwidth shaping** with a leaky-bucket limiter (`--bandwidth-limit`).
- **Automatic preallocation** to reduce fragmentation (uses `fallocate` when available).
- **Optional SHA256 verification** from a digest or checksum file.

## Installation
### windows install
# Download the prebuilt binary
curl -L -o kdownload.zip https://github.com/compiledkernel-idk/kdownload/releases/download/v0.1.0/kdownload-0.1.0-windows-x86_64.zip

# Extract to $HOME\bin (create if it doesn’t exist)
Expand-Archive kdownload.zip -DestinationPath "$HOME\bin" -Force

# Add $HOME\bin to PATH (permanent)
setx PATH "$($env:PATH);$HOME\bin"

# Restart PowerShell, then test
kdownload --help


### Build from source
```bash
# Clone or unpack the sources
cargo build --release
# Install the optimized binary locally
make install PREFIX=/usr/local
```
The Makefile supplies `build`, `install`, and `test` targets and honours the common `PREFIX`/`DESTDIR` conventions.

### Arch Linux (AUR)
The repository ships a `PKGBUILD` ready for the AUR. To try it locally:
```bash
makepkg -si
```
Publishing to the AUR only requires uploading the `PKGBUILD`, `.SRCINFO`, and supporting files.

## Usage

```text
kdownload <url> [<url2> ...]
Options:
  -o, --output <path>       Output path (file or dir)
  -c, --connections <int>   Max connections per host (default: 16)
  -s, --segments <int>      Initial number of segments (default: 16)
  -m, --mirror <url>        Add mirror(s)
      --sha256 <hex|path>   Verify checksum
      --resume              Resume if partial exists
      --timeout <secs>      Per-request timeout
      --bandwidth-limit     Limit speed, e.g. 50M/s
      --unsafe-conn <int>   Allow >16 connections (advanced)
  -q, --quiet               Reduce logging
  -v, --verbose             Increase logging detail
```

Practical examples:

```bash
# Download a large image while keeping logs concise
kdownload "https://example.com/bigfile.iso"

# Resume a partially downloaded file, verifying with a known digest
kdownload --resume --sha256 4d9677f... "https://mirror1/file.iso" -m "https://mirror2/file.iso"

# Limit bandwidth and raise the connection cap explicitly
kdownload --bandwidth-limit 50M/s --unsafe-conn 32 --connections 24 "https://host/file.tar"
```

## How it works

1. `kdownload` probes every URL with a HEAD/range request to discover size and range support.
2. When ranges are available, it preallocates the output file, reconstructs any prior progress from the `.kdl.partmap`, and schedules segments across mirrors.
3. Adaptive scheduling measures per-connection throughput and raises or lowers concurrency to best match network conditions.
4. Each chunk is written directly to the proper offset using `pwrite` semantics, keeping the filesystem consistent even on crashes.
5. On success, the part map is removed and (optionally) a SHA256 check is performed before reporting completion.

## Contributing
Patches and issues are welcome. Please run `cargo fmt` and `cargo test` before submitting pull requests.

## License
Licensed under the MIT License. See [LICENSE](LICENSE) for details.
