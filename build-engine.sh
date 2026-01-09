#!/usr/bin/env bash
# EXSA Engine - Intelligent Hardware Detection & Build System
# Automatically detects GPU/hardware and builds with the optimal backend

set -e
set -o pipefail

COLOR_RED='\033[0;31m'
COLOR_YELLOW='\033[1;33m'
COLOR_GREEN='\033[0;32m'
COLOR_CYAN='\033[0;36m'
COLOR_NC='\033[0m'

info() { echo -e "${COLOR_CYAN}$*${COLOR_NC}"; }
ok() { echo -e "${COLOR_GREEN}$*${COLOR_NC}"; }
warn() { echo -e "${COLOR_YELLOW}$*${COLOR_NC}"; }
err() { echo -e "${COLOR_RED}$*${COLOR_NC}"; }

command_exists() { command -v "$1" >/dev/null 2>&1; }

detect_os_family() {
    local os_type
    os_type="$(uname -s 2>/dev/null || echo unknown)"
    case "$os_type" in
        Darwin) echo "macos" ;;
        Linux) echo "linux" ;;
        MINGW*|MSYS*|CYGWIN*) echo "windows" ;;
        *) echo "unknown" ;;
    esac
}

print_install_instructions() {
    local os_family="$1"

    echo ""
    echo "========================================="
    err "❌ Missing build dependencies"
    echo "========================================="
    echo ""
    echo "The engine builds llama.cpp via the Rust 'cmake' crate, which requires the 'cmake' executable to be installed and on PATH."
    echo ""
    case "$os_family" in
        macos)
            echo "macOS:"
            echo "  - Install Xcode Command Line Tools (compiler toolchain):"
            echo "      xcode-select --install"
            echo "  - Install CMake (recommended via Homebrew):"
            echo "      brew install cmake"
            ;;
        linux)
            echo "Linux:"
            echo "  - Debian/Ubuntu:"
            echo "      sudo apt-get update && sudo apt-get install -y cmake build-essential pkg-config"
            echo "  - Fedora/RHEL:"
            echo "      sudo dnf install -y cmake gcc gcc-c++ make pkgconf-pkg-config"
            echo "  - Arch:"
            echo "      sudo pacman -S --needed cmake base-devel pkgconf"
            ;;
        windows)
            echo "Windows (Git Bash/MSYS2/WSL):"
            echo "  - Install CMake and ensure it's on PATH:"
            echo "      winget install Kitware.CMake"
            echo "    or"
            echo "      choco install cmake"
            echo "  - Ensure you have a C/C++ toolchain available (Visual Studio Build Tools or mingw-w64), or build inside WSL."
            ;;
        *)
            echo "Install CMake from: https://cmake.org/download/"
            ;;
    esac
    echo ""
    echo "After installing, re-run: ./build-engine.sh"
    echo ""
}

try_auto_install_deps() {
    # Opt-in only. Some environments don't allow package installs.
    # Usage: AUTO_INSTALL_DEPS=1 ./build-engine.sh
    if [[ -z "${AUTO_INSTALL_DEPS:-}" ]]; then
        return 1
    fi

    local os_family="$1"
    case "$os_family" in
        macos)
            if command_exists brew; then
                warn "AUTO_INSTALL_DEPS=1: installing 'cmake' via Homebrew..."
                brew install cmake
                return 0
            fi
            return 1
            ;;
        linux)
            if command_exists apt-get; then
                warn "AUTO_INSTALL_DEPS=1: installing build deps via apt-get (requires sudo)..."
                sudo apt-get update
                sudo apt-get install -y cmake build-essential pkg-config
                return 0
            fi
            if command_exists dnf; then
                warn "AUTO_INSTALL_DEPS=1: installing build deps via dnf (requires sudo)..."
                sudo dnf install -y cmake gcc gcc-c++ make pkgconf-pkg-config
                return 0
            fi
            if command_exists pacman; then
                warn "AUTO_INSTALL_DEPS=1: installing build deps via pacman (requires sudo)..."
                sudo pacman -S --needed cmake base-devel pkgconf
                return 0
            fi
            return 1
            ;;
        *)
            return 1
            ;;
    esac
}

ensure_build_deps() {
    local os_family="$1"

    info "🔧 Checking build dependencies..."

    if ! command_exists cargo; then
        err "Missing 'cargo' (Rust toolchain). Install Rust from https://rustup.rs/"
        exit 1
    fi

    # CMake is required by llama-cpp-2's native build.
    if ! command_exists cmake; then
        if try_auto_install_deps "$os_family"; then
            :
        fi

        if ! command_exists cmake; then
            print_install_instructions "$os_family"
            exit 1
        fi
    fi

    # Basic C/C++ toolchain checks (best-effort; some setups use MSVC/WSL).
    if [[ "$os_family" == "macos" ]]; then
        if ! command_exists clang; then
            warn "clang not found. Install Xcode Command Line Tools: xcode-select --install"
        fi
    elif [[ "$os_family" == "linux" ]]; then
        if ! command_exists cc && ! command_exists gcc && ! command_exists clang; then
            warn "No C compiler found (cc/gcc/clang). Install your distro's build tools (e.g. build-essential)."
        fi
    fi

    ok "✓ Dependencies look OK"
}

echo "========================================="
echo "EXSA ENGINE - SMART BUILD SYSTEM"
echo "========================================="
echo ""

# Initialize feature flag
FEATURE_FLAGS=""
BUILD_TYPE="${BUILD_TYPE:-release}"  # Can be overridden: BUILD_TYPE=debug ./build-engine.sh

# ============================================
# DETECTION LOGIC - THE DECISION TREE
# ============================================

detect_hardware() {
    local os_type=$(uname -s)
    local arch=$(uname -m)
    
    echo "🔍 Detecting hardware..."
    echo "   OS: $os_type"
    echo "   Architecture: $arch"
    echo ""
    
    # ========================================
    # Priority 1: macOS ARM64 (Apple Silicon)
    # ========================================
    if [[ "$os_type" == "Darwin" ]] && [[ "$arch" == "arm64" ]]; then
        echo "✅ Detected: Apple Silicon (M1/M2/M3/M4)"
        echo "🚀 Backend: METAL (Native GPU acceleration)"
        FEATURE_FLAGS="metal"
        return 0
    fi
    
    # ========================================
    # Priority 2: macOS x86_64 (Intel Mac)
    # ========================================
    if [[ "$os_type" == "Darwin" ]] && [[ "$arch" == "x86_64" ]]; then
        echo "✅ Detected: Intel Mac"
        
        # Check if NVIDIA eGPU is available
        if command -v nvcc &> /dev/null; then
            echo "🚀 Backend: CUDA (NVIDIA detected via nvcc)"
            FEATURE_FLAGS="cuda"
            
            # Try to detect compute capability
            if command -v nvidia-smi &> /dev/null; then
                GPU_INFO=$(nvidia-smi --query-gpu=name,compute_cap --format=csv,noheader 2>/dev/null | head -1)
                echo "   GPU: $GPU_INFO"
            fi
        else
            echo "⚠️  No NVIDIA GPU detected"
            echo "🚀 Backend: CPU-only (fallback)"
            FEATURE_FLAGS="cpu"
        fi
        return 0
    fi
    
    # ========================================
    # Priority 3: Linux/Windows with NVIDIA CUDA
    # ========================================
    if [[ "$os_type" == "Linux" || "$os_type" == MINGW* || "$os_type" == MSYS* || "$os_type" == CYGWIN* ]]; then
        if command -v nvcc &> /dev/null; then
            NVCC_VERSION=$(nvcc --version | grep "release" | awk '{print $5}' | tr -d ',')
            echo "✅ Detected: NVIDIA CUDA Toolkit"
            echo "   Version: $NVCC_VERSION"
            echo "🚀 Backend: CUDA"
            FEATURE_FLAGS="cuda"
            
            # Detect compute capability for optimization
            if command -v nvidia-smi &> /dev/null; then
                GPU_NAME=$(nvidia-smi --query-gpu=name --format=csv,noheader 2>/dev/null | head -1)
                COMPUTE_CAP=$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1)
                echo "   GPU: $GPU_NAME"
                echo "   Compute Capability: $COMPUTE_CAP"
                
                # Export for potential use by llama.cpp build
                export CUDA_COMPUTE_CAPABILITY="$COMPUTE_CAP"
            fi
            return 0
        fi
    fi
    
    # ========================================
    # Priority 4: Linux with AMD ROCm
    # ========================================
    if [[ "$os_type" == "Linux" ]]; then
        if command -v hipcc &> /dev/null || [[ -d "/opt/rocm" ]]; then
            echo "✅ Detected: AMD ROCm"
            echo "🚀 Backend: ROCm (via hipBLAS)"
            FEATURE_FLAGS="rocm"
            
            if command -v rocminfo &> /dev/null; then
                GPU_INFO=$(rocminfo | grep "Name:" | head -1)
                echo "   $GPU_INFO"
            fi
            return 0
        fi
    fi
    
    # ========================================
    # Priority 5: Vulkan (cross-platform GPU)
    # ========================================
    if command -v vulkaninfo &> /dev/null || [[ -f "/usr/lib/libvulkan.so" ]] || [[ -f "/usr/local/lib/libvulkan.dylib" ]]; then
        echo "⚠️  Detected: Vulkan support"
        echo "🚀 Backend: VULKAN (fallback GPU)"
        FEATURE_FLAGS="vulkan"
        return 0
    fi
    
    # ========================================
    # Fallback: CPU-only
    # ========================================
    echo "⚠️  No GPU acceleration detected"
    echo "🚀 Backend: CPU-only (safe mode)"
    FEATURE_FLAGS="cpu"
}

# ============================================
# BUILD EXECUTION
# ============================================

build_engine() {
    echo ""
    echo "========================================="
    echo "BUILDING EXSA ENGINE"
    echo "========================================="
    echo "Backend: $FEATURE_FLAGS"
    echo "Build Type: $BUILD_TYPE"
    echo ""
    
    # Construct cargo command
    local cargo_cmd="cargo build"
    
    if [[ "$BUILD_TYPE" == "release" ]]; then
        cargo_cmd="$cargo_cmd --release"
    fi
    
    # Add feature flags
    if [[ -n "$FEATURE_FLAGS" ]]; then
        cargo_cmd="$cargo_cmd --no-default-features --features $FEATURE_FLAGS"
    fi
    
    echo "Command: $cargo_cmd"
    echo ""
    
    # Execute build
    if eval "$cargo_cmd"; then
        echo ""
        echo "========================================="
        echo "✅ BUILD SUCCESSFUL"
        echo "========================================="
        echo "Backend: $FEATURE_FLAGS"
        echo "Binary: target/$BUILD_TYPE/exsa-engine"
        echo ""
        
        # Show binary size
        if [[ -f "target/$BUILD_TYPE/exsa-engine" ]]; then
            BINARY_SIZE=$(ls -lh "target/$BUILD_TYPE/exsa-engine" | awk '{print $5}')
            echo "Binary Size: $BINARY_SIZE"
        fi
        
        return 0
    else
        echo ""
        echo "========================================="
        echo "❌ BUILD FAILED"
        echo "========================================="
        echo "Backend: $FEATURE_FLAGS"
        echo ""
        echo "Troubleshooting:"
        echo "  - Ensure CMake is installed (required to build llama.cpp native deps)"
        echo "  - For CUDA: Ensure CUDA toolkit is installed and nvcc is in PATH"
        echo "  - For ROCm: Ensure ROCm is installed in /opt/rocm"
        echo "  - For Metal: Also requires CMake + Xcode CLT on Apple Silicon"
        echo "  - Try fallback: BUILD_TYPE=release FORCE_CPU=1 ./build-engine.sh"
        echo ""
        return 1
    fi
}

# ============================================
# MAIN EXECUTION
# ============================================

main() {
    local os_family
    os_family="$(detect_os_family)"
    ensure_build_deps "$os_family"

    # Check for force override
    if [[ -n "$FORCE_METAL" ]]; then
        echo "🔧 FORCE_METAL=1 - Using Metal backend"
        FEATURE_FLAGS="metal"
    elif [[ -n "$FORCE_CUDA" ]]; then
        echo "🔧 FORCE_CUDA=1 - Using CUDA backend"
        FEATURE_FLAGS="cuda"
    elif [[ -n "$FORCE_ROCM" ]]; then
        echo "🔧 FORCE_ROCM=1 - Using ROCm backend"
        FEATURE_FLAGS="rocm"
    elif [[ -n "$FORCE_CPU" ]]; then
        echo "🔧 FORCE_CPU=1 - Using CPU-only backend"
        FEATURE_FLAGS="cpu"
    else
        # Auto-detect
        detect_hardware
    fi
    
    # Build with detected/forced backend
    build_engine
}

# Run main
main "$@"
