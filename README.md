<!-- markdownlint-configure-file {
  "MD033": false,
  "MD041": false
} -->

<div align="center">

# otaripper

Extract partitions from Android OTA files with cryptographic verification, strong reliability guarantees, and high-performance execution.

[![Crates.io](https://img.shields.io/crates/v/otaripper?style=for-the-badge&logo=rust&logoColor=white&label=crates.io&color=rust)](https://crates.io/crates/otaripper)
[![GitHub release](https://img.shields.io/github/v/release/syedinsaf/otaripper?style=for-the-badge&logo=github&logoColor=white&color=rust)](https://github.com/syedinsaf/otaripper/releases)
[![Downloads](https://img.shields.io/github/downloads/syedinsaf/otaripper/total?style=for-the-badge&logo=github&logoColor=white&color=rust)](https://github.com/syedinsaf/otaripper/releases)
[![License](https://img.shields.io/github/license/syedinsaf/otaripper?style=for-the-badge&logo=github&logoColor=white&color=rust)](LICENSE)


[Download](https://github.com/syedinsaf/otaripper/releases) •
[Quick Start](#quick-start) •
[Build Guide](#building-from-source) •
[Technical Details](TECHNICAL.md)

</div>

---

## Table of Contents

* [Overview](#overview)
* [Feature Comparison](#feature-comparison)
* [Performance](#performance)
* [Quick Start](#quick-start)
* [Basic Usage](#basic-usage)
* [Cleanup](#cleanup)
* [Command Options](#command-options)
* [Building from Source](#building-from-source)
* [Contributing](#contributing)
* [Acknowledgments](#acknowledgments)
* [Show Your Support](#show-your-support)
* [License](#license)
* [Disclaimer](#disclaimer)

For in-depth architecture and performance details, see [TECHNICAL.md](TECHNICAL.md)

---

## Overview

**otaripper** extracts partitions from Android OTA packages (`payload.bin` or full OTA `.zip` files).

The tool is written in Rust and prioritizes:

* Cryptographic correctness and data integrity
* Predictable, fail-safe behavior
* High-performance, multi-threaded execution
* SIMD-accelerated memory operations
* Guaranteed cleanup on failure or interruption

Unlike many extraction tools, otaripper **verifies output images by default** and refuses to leave behind partially valid or corrupted files.

⚠️ Incremental OTA packages are intentionally **not supported**.

---

## Feature Comparison

|                         | otaripper v3.1 | payload-dumper-go | payload_dumper (Python) |
| ----------------------- | -------------- | ----------------- | ----------------------- |
| Output verification     | ✅ SHA-256      | ❌                | ❌                      |
| Remote HTTP Streaming   | ✅ (Parallel)   | ❌                | ❌                      |
| SIMD optimization       | ✅ AVX-512 / AVX2 / SSE2 | ❌        | ❌                      |
| Cache-aware large writes| ✅              | ❌                | ❌                      |
| Graceful interruption   | ✅              | ❌                | ❌                      |
| Auto-cleanup on failure | ✅              | ❌                | ❌                      |
| Performance statistics  | ✅              | ❌                | ❌                      |
| Selective extraction    | ✅              | ✅                | ✅                      |
| Direct ZIP Memory Mapping| ✅ (Zero-Copy)  | ❌ (Extracts temp)| ❌                      |
| Multi-threaded          | ✅              | ✅                | ❌ (single-threaded)    |
| Cross-platform          | ✅              | ✅                | ⚠️ Requires Python     |
| Standalone binary       | ✅              | ✅                | ❌                      |

> otaripper is designed to fail early and cleanly rather than produce questionable output.

---

## Performance

otaripper automatically detects CPU capabilities and selects the optimal execution path.

Version **3.1** introduces parallel chunked Remote HTTP Streaming and massive I/O savings:

* **Parallel HTTP Streaming**: Extract specific partitions directly from a remote URL! otaripper intelligently streams only the required byte-ranges over the network, using multiple parallel 8MB chunked requests to saturate your bandwidth.
* **Network Resilience**: Built-in automatic retries with exponential backoff and real-time offline detection. Never fail an extraction due to a temporary connection drop again.
* **Direct ZIP Memory Mapping**: Bypasses the traditional temp-file extraction step for `STORED` OTA zips, mapping the internal `payload.bin` straight from the disk using a zero-copy offset.
* **Modern Decompression Engine**: Upgraded `liblzma` backend safely handles modern Android payloads utilizing the ARM64 BCJ filter (e.g., Xiaomi HyperOS).
* **Modular Engine Architecture**: Breaking the monolithic extraction logic into specialized `extractor` and `simd` modules.
* **Thread-Local Buffer Pooling**: Drastically amortizing memory allocations across deep Rayon threadpools.
* **Zero-Copy Decompression**: Triggering purely alloc-free extraction paths when output blocks map cleanly to continuous extents.
* **Strict SIMD Encapsulation**: Cleanly isolating CPU vector operations (`AVX-512`, `AVX2`, `SSE2`) through non-temporal cache-bypassing mechanisms.

```

Throughput Example (3GB system partition)
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
otaripper (AVX-512)  ████████████ 2.8 GB/s
otaripper (AVX2)     ████████     1.9 GB/s
payload-dumper-go    ████         1.0 GB/s
payload_dumper       ██           0.4 GB/s
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

```

Performance scales with:

* storage speed (NVMe > SATA > HDD)
* compression format
* CPU SIMD capability

For architectural details, see [TECHNICAL.md](TECHNICAL.md)

---

## Quick Start

### Installation

Prebuilt binaries are available on the
[Releases](https://github.com/syedinsaf/otaripper/releases) page:

* Windows: `otaripper-x86_64-pc-windows-msvc.exe`
* Linux (glibc): `otaripper-x86_64-unknown-linux-gnu`
* Linux (musl): `otaripper-x86_64-unknown-linux-musl`
* macOS (Intel): `otaripper-x86_64-apple-darwin`
* macOS (Apple Silicon): `otaripper-aarch64-apple-darwin`

> **Note:** Each release also includes `otaripper-lite` binaries. These are compiled without remote HTTP extraction features (`--no-default-features`), resulting in an ultra-minimal footprint for users who only extract local files.

### Verifying Downloads

otaripper releases follow a "Gold Standard" two-layer checksum architecture:
1. **Download Verification:** The release page hosts a master `checksums.txt` file containing hashes for all `.tar.gz` and `.zip` archives.
2. **Binary Verification:** Upon extracting the archive, you will find an `otaripper-vX.Y.Z.sha256` file enclosed alongside the executables. Run `sha256sum -c *.sha256` in your terminal to instantly verify the integrity of the extracted binaries.

### Windows (winget)

otaripper is available via the Windows Package Manager:

```powershell
winget install syedinsaf.otaripper
```

To update to the latest version:

```powershell
winget update syedinsaf.otaripper
# or
winget upgrade syedinsaf.otaripper
```

## Arch Linux (AUR)

otaripper is available on the AUR:

```bash
paru -S otaripper        # build from source (recommended)
paru -S otaripper-bin    # prebuilt glibc binary

yay -S otaripper
yay -S otaripper-bin
```

If `otaripper-bin` fails to run due to libc/runtime issues, use the
fully static musl build from GitHub Releases:

[https://github.com/syedinsaf/otaripper/releases](https://github.com/syedinsaf/otaripper/releases)

---

## Basic Usage

Extract everything:

```bash
otaripper ota.zip
````

List partitions:

```bash
otaripper -l ota.zip
```

Extract selected partitions:

```bash
otaripper ota.zip -p boot,vendor_boot,init_boot
```

🌐 **Remote HTTP Streaming (Zero-Download Extraction):**

Extract specific partitions directly from a web URL without downloading the full OTA package!

```bash
otaripper https://android.googleapis.com/packages/ota-api/package.zip -p boot,init_boot
```

Print hashes:

```bash
otaripper ota.zip --print-hash
```

Strict verification:

```bash
otaripper ota.zip --strict
```

Disable automatic folder opening:

```bash
otaripper ota.zip -n
```

Analyze Qualcomm bootloader Anti-Rollback (ARB) metadata (accepts `.img`, `.bin`, or `.zip`):

```bash
otaripper arb update.zip
```

🌐 **Remote ARB Inspection (Zero-Download) & Interactive Export:**

Instantly check the ARB index of a firmware update without downloading the massive 3GB+ zip file! `otaripper` will intelligently stream and extract just the tiny `xbl_config.img` directly from the URL over the internet. You can optionally export the metadata into a beautifully formatted, platform-sanitized JSON file named dynamically after your device model, software build, and ARB index:

```text
$ otaripper arb https://example.com/firmware.zip
[arbscan] OTA package detected. Extracting xbl_config.img temporarily...
[arbscan] Analyzing: xbl_config.img (from https://example.com/firmware.zip)

OEM Metadata
────────────
  Major Version : 3
  Minor Version : 0
  ARB Index     : 0

Write JSON output? [y/N]: y
Device model      : op12
Update / build    : 10.1.100

✔ JSON written: op12_10.1.100_ARB(0).json
```

---

## Cleanup

Remove previously extracted folders:

```bash
otaripper clean
```

Clean a specific directory:

```bash
otaripper clean -o /path/to/output
```

The cleanup command only removes directories matching `extracted_*`
and refuses to operate on filesystem roots for safety.

---

## Command Options

| Option             | Description                         |
| ------------------ | ----------------------------------- |
| `-l, --list`       | List partitions only                |
| `-p, --partitions` | Extract specific partitions         |
| `-o, --output-dir` | Custom output directory             |
| `--strict`         | Enforce manifest hashes             |
| `--no-verify`      | Disable verification (unsafe)       |
| `--print-hash`     | Print SHA-256 hashes                |
| `--sanity`         | Detect obviously invalid output     |
| `--stats`          | Show performance statistics         |
| `-t, --threads`    | Thread control (1–256, 0 = auto)    |
| `-n, --no-open`    | Disable folder auto-open            |
| `clean`            | Remove `extracted_*` folders safely |
| `arbscan`, `arb`   | Extract ARB metadata from bootloader images or payloads |

---

## Building from Source

### Requirements

* **Rust 1.95.0 or newer** (MSRV)
* Git
* C compiler (gcc / clang / MSVC) - required by some native dependencies

### Build

We provide two built-in Cargo aliases for easy compilation:

```bash
git clone https://github.com/syedinsaf/otaripper.git
cd otaripper

# Build the standard CLI (with Remote HTTP Streaming support)
cargo full

# Build the 'lite' CLI (Network-free, local extraction only)
cargo lite
```

**Binary output:**

* Full Build: `target/release/otaripper`
* Lite Build: `target/lite/release/otaripper`

> **Note:** The `cargo lite` alias automatically isolates its output into a separate `target/lite/` directory. This ensures you can compile and test both versions side-by-side locally without them overwriting each other!

---


## Native Optimized Build (Advanced)

otaripper can be built locally with **CPU-specific optimizations** for maximum performance.
This enables all instruction sets supported by your CPU (AVX2 / AVX-512 / ARMv8, etc.).

⚠️ **Important:**
Binaries built this way are **NOT portable** and **must NOT be redistributed**.

---

### Linux / macOS (build.sh)

A helper script is provided to:
- download the source
- optionally install Rust (with confirmation)
- build a **CPU-native release binary**
- clean up all intermediate files

#### Requirements
- `curl`
- `unzip`
- A C toolchain (gcc / clang)
- Rust (installed automatically if missing)

#### Usage

```bash
chmod +x build.sh
./build.sh
````

Output binary:
After running `build.sh`, a new folder named `otaripper-native`
will be created **in the same directory where `build.sh` is located**.


```text
~/otaripper-native/otaripper
```

---

### Windows (PowerShell – MSVC)

On Windows, a native PowerShell script is provided.
It uses the official **Windows rustup installer** and defaults to the **MSVC toolchain**.

#### Requirements

* Windows 10 / 11
* PowerShell 5.1 or newer
* Visual Studio Build Tools (prompted automatically if missing)

#### Usage

Before running the script, allow execution **for the current session only**:

```powershell
Set-ExecutionPolicy -Scope Process -ExecutionPolicy Bypass
```

Then run:

```powershell
.\build.ps1
```

Output binary:
After running `build.ps1`, a new folder named `otaripper-native`
will be created **in the same directory where `build.ps1` is located**.

```text
otaripper-native\otaripper.exe
```

---

### Notes

* Native builds use `-C target-cpu=native`
* Performance may be significantly higher than portable binaries
* These builds are intended for **local use only**
* GitHub Releases remain the recommended option for most users

---

## Contributing

Testing, bug reports, and performance feedback are welcome.

Please include:

* OS, CPU, RAM
* otaripper version 3.1.0
* OTA size and format
* logs or error messages if available

Pull requests should:

1. Build cleanly
2. Preserve safety guarantees
3. Avoid introducing undefined behavior
4. Keep performance regressions justified

---

## Acknowledgments

otaripper benefits greatly from real-world testing and feedback.

Special thanks to **Jean Rivera** for extensive validation, edge-case testing,
and correctness feedback.

Thanks also to:

* Android Open Source Project documentation
* Rust ecosystem maintainers
* Users who reported bugs and performance issues

---

## Show Your Support

If otaripper helped you:

* **Star the repository** — https://github.com/syedinsaf/otaripper
* **Report issues** — https://github.com/syedinsaf/otaripper/issues
* **Submit pull requests** — https://github.com/syedinsaf/otaripper/pulls
* **Share with others** — https://github.com/syedinsaf/otaripper

---

## License

otaripper is licensed under the **Apache License 2.0**.
See [LICENSE](LICENSE) for details.

---

## Disclaimer

Use at your own risk.

* Always verify extracted images before flashing
* Keep backups whenever possible
* Understand your device and bootloader requirements

The author and contributors are not responsible for data loss,
bricked devices, or damage resulting from misuse.

---
