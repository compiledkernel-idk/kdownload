# kdownload

`kdownload` is a blazing-fast, async command-line downloader for Linux and Windows. It uses segmented range requests, adaptive scheduling, and mirror-aware balancing to squeeze as much bandwidth as possible out of every host.

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
You can either ```makepkg``` it manually or download it with an AUR helper
```bash
yay -S kdownload
```
or

```bash
paru -S kdownload
```

add **-bin** if you don't want to compile it from source


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
  -c, --connections <int>   Max connections per host (default: 32)
  -s, --segments <int>      Initial number of segments (default: 64)
  -m, --mirror <url>        Add mirror(s)
      --sha256 <hex|path>   Verify checksum
      --resume              Resume if partial exists
      --timeout <secs>      Per-request timeout
      --bandwidth-limit     Limit speed, e.g. 50M/s
      --unsafe-conn <int>   Allow >32 connections (advanced)
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

Performance comparison against standard tools on a 100MB file download (v1.4.0). Tests conducted using `http://speedtest.tele2.net/100MB.zip`.

| Tool | Time (s) | Speed (approx.) |
| :--- | :--- | :--- |
| **kdownload v1.4.0** | **3.50s** | **29 MB/s** |
| aria2c (16 conns) | 8.09s | 12 MB/s |
| aria2c (1 conn) | 19.05s | 3.6 MB/s |
| wget | ~26s | ~3 MB/s |
| curl | ~25s | ~3 MB/s |

**Key observations:**
- **kdownload v1.4.0** delivers ~2.3x faster downloads compared to optimized `aria2c` (16 connections).
- Massive performance gap against single-connection tools (wget/curl/aria2c default), achieving **~7-8x speedup**.
- Zero-allocation buffer pool and non-blocking I/O allow it to saturate bandwidth efficiently.

To reproduce these benchmarks:
```bash
./benchmark.sh
```

The benchmark script compares download speeds across different file sizes and generates a CSV report in `benchmark_results/`.

## Contributing
Patches and issues are welcome. Please run `cargo fmt` and `cargo test` before submitting pull requests.

## License
Licensed under the MIT License. See [LICENSE](LICENSE) for details.
