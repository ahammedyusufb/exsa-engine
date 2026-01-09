# EXSA Engine - Makefile
# Provides easy build targets with automatic hardware detection

.PHONY: build release debug clean test metal cuda vulkan cpu help

# Default target - auto-detect hardware and build
build: release

# Auto-detect hardware and build release
release:
	@chmod +x build-engine.sh
	@BUILD_TYPE=release ./build-engine.sh

# Auto-detect hardware and build debug
debug:
	@chmod +x build-engine.sh
	@BUILD_TYPE=debug ./build-engine.sh

# ============================================
# Manual Override Targets
# ============================================

# Force Metal build (Apple Silicon)
metal:
	@echo "🔧 Manual override: Building with Metal backend"
	@cargo build --release --no-default-features --features metal

# Force CUDA build (NVIDIA)
cuda:
	@echo "🔧 Manual override: Building with CUDA backend"
	@cargo build --release --no-default-features --features cuda

# Force Vulkan build (cross-platform GPU)
vulkan:
	@echo "🔧 Manual override: Building with Vulkan backend"
	@cargo build --release --no-default-features --features vulkan

# Force CPU-only build (no GPU)
cpu:
	@echo "🔧 Manual override: Building CPU-only"
	@cargo build --release --no-default-features --features cpu

# ============================================
# Development Targets
# ============================================

# Run tests
test:
	@cargo test --lib --bins

# Clean build artifacts
clean:
	@./clean.sh

# Aggressive clean (includes cargo cache)
clean-all:
	@./clean.sh --aggressive

# Super aggressive clean (git clean -fdx)
clean-super:
	@./clean.sh --super-aggressive

# Check code without building
check:
	@cargo check --all-targets

# Format code
fmt:
	@cargo fmt

# Run clippy linter
clippy:
	@cargo clippy --all-targets

# ============================================
# Utility Targets
# ============================================

# Show help
help:
	@echo "EXSA Engine - Build System"
	@echo ""
	@echo "Auto-Detection Targets:"
	@echo "  make build    - Auto-detect hardware and build (default)"
	@echo "  make release  - Auto-detect hardware and build release"
	@echo "  make debug    - Auto-detect hardware and build debug"
	@echo ""
	@echo "Manual Override Targets:"
	@echo "  make metal    - Force Metal build (Apple Silicon)"
	@echo "  make cuda     - Force CUDA build (NVIDIA)"
	@echo "  make vulkan   - Force Vulkan build (cross-platform)"
	@echo "  make cpu      - Force CPU-only build"
	@echo ""
	@echo "Development Targets:"
	@echo "  make test     - Run test suite"
	@echo "  make clean    - Clean build artifacts"
	@echo "  make check    - Check code without building"
	@echo "  make fmt      - Format code"
	@echo "  make clippy   - Run linter"
	@echo ""
	@echo "Hardware Detection Script:"
	@echo "  ./build-engine.sh              - Auto-detect and build"
	@echo "  FORCE_METAL=1 ./build-engine.sh  - Force Metal"
	@echo "  FORCE_CUDA=1 ./build-engine.sh   - Force CUDA"
	@echo "  FORCE_CPU=1 ./build-engine.sh    - Force CPU-only"
