# Universal Build System - Hardware Detection & Feature Selection

## The Core Problem: The "Chicken and Egg" Dilemma

**Critical Understanding**: You cannot use a Rust build script (`build.rs`) to enable features like "CUDA" or "Metal" because `build.rs` runs **after** Cargo has already decided which features to enable.

### Wrong Approach ❌
Writing a `build.rs` that checks for NVIDIA GPU and tries to enable the `cuda` feature.
- **Problem**: Too late! Cargo has already compiled the dependency tree without it.

### Correct Approach ✅
A **Pre-Build Detector** (Shell Script) that scans the hardware **first**, then tells Cargo exactly what to do.

---

## Implementation Architecture

### Component A: The Feature Gateway (`Cargo.toml`)

Your `Cargo.toml` acts as a **Switchboard** - it doesn't care what hardware exists; it just provides wires that can be plugged in.

**Logic**:
1. Disable default features for `llama-cpp-2` (no guessing)
2. Create explicit "Passthrough" features
3. When you enable `exsa-engine/cuda`, it automatically turns on `llama-cpp-2/cuda`

```toml
[package]
name = "exsa-engine"
version = "0.1.0"

[dependencies]
# Disable defaults - we'll manually select the backend
llama-cpp-2 = { version = "0.1", default-features = false }

[features]
# Passthrough features - each maps to llama-cpp-2's backend
metal = ["llama-cpp-2/metal"]
cuda = ["llama-cpp-2/cuda"]
rocm = ["llama-cpp-2/hipblas"]  # AMD backend
vulkan = ["llama-cpp-2/vulkan"]
cpu = []  # CPU-only fallback
```

---

### Component B: The Hardware Sniffer (`build-engine.sh`)

This script implements the **Intelligence Layer** with a specific decision tree:

#### Decision Tree Logic:
```
1. Check OS: Is it macOS?
   └─ Yes → Is it ARM64 (M1/M2/M3)?
      └─ Yes → FORCE METAL ✅

2. Check Compiler: Is nvcc (NVIDIA) in $PATH?
   └─ Yes → FORCE CUDA ✅

3. Check Driver: Is hipcc or /opt/rocm present?
   └─ Yes → FORCE ROCm ✅

4. Fallback: Use VULKAN or CPU (Safe mode)
```

#### Implementation (`build-engine.sh`):

```bash
#!/usr/bin/env bash
set -e

echo "🔍 Detecting hardware..."

FEATURE_FLAGS=""

# 1. Check for Apple Silicon (M1/M2/M3)
if [[ "$(uname -s)" == "Darwin" ]] && [[ "$(uname -m)" == "arm64" ]]; then
    echo "✅ Detected: Apple Silicon (Metal)"
    FEATURE_FLAGS="metal"

# 2. Check for NVIDIA CUDA
elif command -v nvcc &> /dev/null; then
    CUDA_VERSION=$(nvcc --version | grep "release" | awk '{print $5}' | cut -d',' -f1)
    echo "✅ Detected: NVIDIA CUDA $CUDA_VERSION"
    FEATURE_FLAGS="cuda"
    
    # Optional: Set compute capability
    export CUDA_COMPUTE_CAPABILITY="8.6"  # Adjust for your GPU

# 3. Check for AMD ROCm
elif command -v hipcc &> /dev/null || [[ -d "/opt/rocm" ]]; then
    echo "✅ Detected: AMD ROCm"
    FEATURE_FLAGS="rocm"

# 4. Check for Vulkan
elif command -v vulkaninfo &> /dev/null; then
    echo "✅ Detected: Vulkan support"
    FEATURE_FLAGS="vulkan"

# 5. Fallback to CPU
else
    echo "⚠️  No GPU detected - using CPU"
    FEATURE_FLAGS="cpu"
fi

echo "🔨 Building with features: $FEATURE_FLAGS"
cargo build --release --no-default-features --features "$FEATURE_FLAGS"

echo "✅ Build complete!"
```

---

### Component C: Advanced - Missing Library Handling

#### For Metal:
- Usually "just works" on macOS (standardized framework paths)
- Frameworks are auto-linked by the system

#### For CUDA:
May need additional environment variables:

```bash
# In build-engine.sh, before cargo build:
if [[ "$FEATURE_FLAGS" == "cuda" ]]; then
    export CUDA_HOME="/usr/local/cuda"
    export LD_LIBRARY_PATH="$CUDA_HOME/lib64:$LD_LIBRARY_PATH"
    export PATH="$CUDA_HOME/bin:$PATH"
fi
```

#### For ROCm:
```bash
if [[ "$FEATURE_FLAGS" == "rocm" ]]; then
    export ROCM_PATH="/opt/rocm"
    export LD_LIBRARY_PATH="$ROCM_PATH/lib:$LD_LIBRARY_PATH"
fi
```

---

## Distribution Strategy: Makefile Wrapper

Don't tell users "Run the script" - wrap it in a standard `Makefile`:

```makefile
# Makefile

# Default: Auto-detect and build
.PHONY: build
build:
	@chmod +x build-engine.sh
	@./build-engine.sh

# Manual overrides for testing
.PHONY: metal cuda rocm vulkan cpu
metal:
	cargo build --release --no-default-features --features metal

cuda:
	cargo build --release --no-default-features --features cuda

rocm:
	cargo build --release --no-default-features --features rocm

vulkan:
	cargo build --release --no-default-features --features vulkan

cpu:
	cargo build --release --no-default-features --features cpu

# Clean build
.PHONY: clean
clean:
	cargo clean

# Run after build
.PHONY: run
run: build
	./target/release/exsa-engine
```

---

## Why This Architecture is Superior

### 1. **Portability** ✅
```bash
# Works everywhere:
git clone https://github.com/you/exsa-engine
cd exsa-engine
make build  # Auto-detects: H100? CUDA. M3? Metal. AMD? ROCm.
```

### 2. **Safety** ✅
- No accidental CPU-only builds running at 2 tokens/second
- Explicit feature selection
- Clear error messages

### 3. **Standardization** ✅
- No manual `Cargo.toml` editing
- Standard `make` interface
- CI/CD friendly

### 4. **Flexibility** ✅
```bash
# Auto-detect (recommended):
make build

# Manual override (testing):
make cuda
make metal
```

---

## Complete Implementation Checklist

- [ ] Edit `Cargo.toml`: Remove default features, add passthrough features
- [ ] Create `build-engine.sh`: Implement detection logic
- [ ] Create `Makefile`: Wrap script for ease of use
- [ ] Test on multiple platforms:
  - [ ] macOS (Apple Silicon)
  - [ ] Linux (NVIDIA)  
  - [ ] Linux (AMD)
  - [ ] Linux (CPU-only)
- [ ] Update `README.md`: Document build process
- [ ] Add CI/CD: Test multiple backends automatically

---

## Example README Section

```markdown
## Building

### Quick Start (Auto-Detection)
```bash
make build
```

The build system automatically detects your hardware:
- **Apple Silicon (M1/M2/M3)**: Builds with Metal
- **NVIDIA GPU**: Builds with CUDA
- **AMD GPU**: Builds with ROCm
- **No GPU**: Builds CPU-only

### Manual Backend Selection
```bash
make metal  # Force Metal
make cuda   # Force CUDA
make rocm   # Force AMD
```

---

## Advanced: CI/CD Matrix

```yaml
# .github/workflows/build.yml
strategy:
  matrix:
    backend: [metal, cuda, rocm, cpu]
    include:
      - backend: metal
        os: macos-latest
      - backend: cuda
        os: ubuntu-latest
        gpu: nvidia
      - backend: rocm
        os: ubuntu-latest
        gpu: amd
      - backend: cpu
        os: ubuntu-latest

steps:
  - run: make ${{ matrix.backend }}
```

---

**Bottom Line**: This system eliminates the "works on my machine" problem by making hardware detection explicit, automatic, and overridable.
