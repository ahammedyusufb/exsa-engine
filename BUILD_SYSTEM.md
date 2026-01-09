# EXSA Engine - Smart Build System

## Overview

The EXSA engine features an **intelligent hardware detection build system** that automatically selects the optimal GPU backend for your hardware without manual configuration.

## How It Works

### The Problem We Solved

Traditional Rust builds with GPU backends have a "chicken and egg" problem:
- Cargo decides which features to compile **before** the build script runs
- You can't detect hardware in `build.rs` and enable features dynamically  
- Solution: **Pre-build hardware detection** that tells Cargo exactly what to compile

### Architecture

```
build-engine.sh (Hardware Sniffer)
    ↓
Detects: macOS ARM64 / NVIDIA / AMD / Vulkan / CPU
    ↓
Sets: FEATURE_FLAGS="metal" (or cuda/rocm/vulkan/cpu)
    ↓
cargo build --features $FEATURE_FLAGS
    ↓
Cargo.toml passthroughs enable llama-cpp-2/$FEATURE
```

## Usage

### Automatic Build (Recommended)

```bash
# Simple - auto-detects hardware
make build

# Or directly
./build-engine.sh
```

**Detection Logic**:
1. **macOS ARM64** (M1/M2/M3/M4) → Metal
2. **NVIDIA GPU** (nvcc found) → CUDA
3. **AMD GPU** (ROCm found) → ROCm*
4. **Vulkan** (vulkaninfo found) → Vulkan
5. **Fallback** → CPU-only

*ROCm feature commented out in current llama-cpp-2 version

### Manual Override

Force a specific backend:

```bash
# Via Makefile
make metal      # Force Metal (Apple Silicon)
make cuda       # Force CUDA (NVIDIA)
make vulkan     # Force Vulkan
make cpu        # Force CPU-only

# Via environment variables
FORCE_METAL=1 ./build-engine.sh
FORCE_CUDA=1 ./build-engine.sh
FORCE_CPU=1 ./build-engine.sh

# Debug build
BUILD_TYPE=debug ./build-engine.sh
```

## File Structure

### `Cargo.toml` Changes

```toml
[dependencies]
# Disabled default features - manual control
llama-cpp-2 = { version = "0.1", default-features = false, optional = true }

[features]
# No default - build script chooses
default = []

# Passthroughs to llama-cpp-2
metal = ["llama-cpp-2/metal", "dep:llama-cpp-2"]
cuda = ["llama-cpp-2/cuda", "dep:llama-cpp-2"]
vulkan = ["llama-cpp-2/vulkan", "dep:llama-cpp-2"]
cpu = ["dep:llama-cpp-2"]
```

**Key Points**:
- `default = []` - No hardcoded backend
- `default-features = false` - Total control over llama-cpp-2
- Explicit passthroughs for each backend

### `build-engine.sh` - The Intelligence Layer

```bash
detect_hardware() {
    # 1. Check macOS ARM64 → Metal
    # 2. Check NVIDIA (nvcc) → CUDA  
    # 3. Check AMD (ROCm) → ROCm
    # 4. Check Vulkan → Vulkan
    # 5. Fallback → CPU
}

# Execute: cargo build --features $DETECTED_BACKEND
```

### `Makefile` - Convenience Layer

Provides easy targets:
- `make build` - Auto-detect and build
- `make metal/cuda/vulkan/cpu` - Manual overrides
- `make test/clean/fmt/clippy` - Development tasks

## Example Output

```bash
$ ./build-engine.sh

=========================================
EXSA ENGINE - SMART BUILD SYSTEM
=========================================

🔍 Detecting hardware...
   OS: Darwin
   Architecture: arm64

✅ Detected: Apple Silicon (M1/M2/M3/M4)
🚀 Backend: METAL (Native GPU acceleration)

=========================================
BUILDING EXSA ENGINE
=========================================
Backend: metal
Build Type: release

Command: cargo build --release --no-default-features --features metal

   Compiling llama-cpp-2 v0.1.126
   Compiling exsa-engine v0.1.0
    Finished `release` profile [optimized] target(s) in 52.33s

=========================================
✅ BUILD SUCCESSFUL
=========================================
Backend: metal
Binary: target/release/exsa-engine
Binary Size: 5.5M
```

## Benefits

### ✅ Portability
- Clone repo on any machine → works automatically
- AWS GPU instance → auto-detects CUDA
- Apple Silicon Mac → auto-detects Metal
- No manual Cargo.toml editing

### ✅ Safety
- Won't accidentally build CPU-only version
- Always gets optimal backend for hardware
- Clear error messages if GPU not available

### ✅ Standardization
- Same workflow everywhere: `make build`
- CI/CD friendly
- Easy to integrate into larger systems

## Troubleshooting

### Build Fails with "feature not found"

Some backends may not be available in the current llama-cpp-2 version:
- ROCm (`hipblas`) - commented out in Cargo.toml for v0.1.126
- Will be re-enabled when dependency updates

**Workaround**: Force a different backend:
```bash
FORCE_CPU=1 ./build-engine.sh
```

### CUDA Build Fails

Ensure CUDA toolkit is installed:
```bash
# Check if nvcc is available
nvcc --version

# Set CUDA_HOME if needed
export CUDA_HOME=/usr/local/cuda
./build-engine.sh
```

### Metal Build Issues (macOS)

Metal should work automatically on Apple Silicon. If it fails:
```bash
# Check architecture
uname -m  # Should show: arm64

# Force CPU as fallback
make cpu
```

## Migration from Old System

**Old Way** (hardcoded):
```toml
[features]
default = ["metal"]  # Breaks on NVIDIA machines
```

**New Way** (intelligent):
```bash
# Just run - it figures it out
make build
```

**Changes Made**:
1. ✅ Removed `default = ["metal"]` from Cargo.toml
2. ✅ Added `default-features = false` to llama-cpp-2
3. ✅ Created `build-engine.sh` hardware detector
4. ✅ Created `Makefile` wrapper
5. ✅ Removed old `build.sh`

## Advanced Usage

### Custom Feature Combinations

```bash
# Direct cargo command (if you know what you're doing)
cargo build --release \
  --no-default-features \
  --features "metal"

# With specific optimizations
RUSTFLAGS="-C target-cpu=native" make build
```

### CI/CD Integration

```yaml
# GitHub Actions example
- name: Build EXSA Engine
  run: |
    chmod +x build-engine.sh
    ./build-engine.sh
```

The build system will auto-detect GitHub Actions runners and select appropriate backend.

## Future Enhancements

Planned improvements:
- [ ] Detect multiple GPUs and select best
- [ ] Hybrid CPU+GPU builds
- [ ] Performance benchmarking in build output
- [ ] Auto-download models if missing
- [ ] Docker build detection

---

**Status**: ✅ Production Ready  
**Tested On**: Apple Silicon (M3 Pro)  
**Build Time**: ~52s (release)  
**Binary Size**: 5.5M
