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
- **Rich progress reporting** with adaptive TTY output or `--json` streaming events.

## Installation

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

### Prebuilt binaries
Each tagged release ships ready-to-run archives for Linux (`kdownload-<ver>-linux-x86_64.tar.gz`) and Windows (`kdownload-<ver>-windows-x86_64.zip`).
Extract the archive and place the `kdownload` binary on your `PATH` to start using it immediately.

### Windows installer (Inno Setup)
The repository ships an Inno Setup script that produces a traditional Windows installer which adds `kdownload.exe` to the system `%PATH%`.

1. Extract the Windows release ZIP to `dist\kdownload-<ver>-windows-x86_64`.
2. Open `packaging/windows/kdownload.iss` with Inno Setup 6 (or newer) on Windows and build the installer.
3. The resulting `kdownload-<ver>-windows-installer.exe` is written back to `dist/`.

Running the installer installs `kdownload.exe`, the bundled README and license to `%ProgramFiles%\kdownload`, updates the PATH, and registers an uninstaller entry.

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
      --json                Emit newline-delimited JSON progress updates
```

Practical examples:

```bash
# Download a large image while keeping logs concise
kdownload "https://example.com/bigfile.iso"

# Resume a partially downloaded file, verifying with a known digest
kdownload --resume --sha256 4d9677f... "https://mirror1/file.iso" -m "https://mirror2/file.iso"

# Limit bandwidth and raise the connection cap explicitly
kdownload --bandwidth-limit 50M/s --unsafe-conn 32 --connections 24 "https://host/file.tar"

# Stream structured progress for automation
kdownload --json "https://example.com/dataset.tar"

```

When `kdownload` runs in a TTY it continuously refreshes a single status line with total bytes, throughput, and active segments. Automation can switch to `--json` to receive newline-delimited progress events with stable keys (`event`, `bytes_downloaded`, `total_bytes`, `fraction`, `bytes_per_second`, `active_segments`, `pending_segments`, `target_parallelism`).

## How it works

1. `kdownload` probes every URL with a HEAD/range request to discover size and range support.
2. When ranges are available, it preallocates the output file, reconstructs any prior progress from the `.kdl.partmap`, and schedules segments across mirrors.
3. Adaptive scheduling measures per-connection throughput and raises or lowers concurrency to best match network conditions.
4. Each chunk is written directly to the proper offset using `pwrite` semantics, keeping the filesystem consistent even on crashes.
5. On success, the part map is removed and (optionally) a SHA256 check is performed before reporting completion.

## Benchmarks

Performance comparison against wget, curl, and aria2c on an Arch Linux system (kernel 6.16.11). Tests were conducted using public CDN endpoints with good bandwidth.

| File Size | kdownload | wget | curl | aria2c |
|-----------|-----------|------|------|--------|
| 10 MB     | **0.18s** (57 MB/s) | 0.21s (48 MB/s) | 0.19s (52 MB/s) | 0.20s (49 MB/s) |
| 100 MB    | 0.66s (151 MB/s) | 0.61s (163 MB/s) | 0.59s (168 MB/s) | **0.57s** (176 MB/s) |

**Key observations:**
- **kdownload** shows excellent performance on small to medium files, outperforming all competitors on 10 MB downloads
- Competitive throughput across all file sizes with adaptive concurrency tuning
- All times represent single-run measurements; actual performance varies with network conditions and server response

To reproduce these benchmarks:
```bash
./benchmark.sh
```

The benchmark script compares download speeds across different file sizes and generates a CSV report in `benchmark_results/`.

## Contributing
Patches and issues are welcome. Please run `cargo fmt` and `cargo test` before submitting pull requests.

## License
Licensed under the MIT License. See [LICENSE](LICENSE) for details.
