# 🕵️ INSIDE THE BUILD SCRIPT: HOW IT WORKS

## The Logic Flow

### 1. Detects Your OS & Architecture
```bash
OS="$(uname -s)"   # e.g., "Darwin" (Mac) or "Linux"
ARCH="$(uname -m)" # e.g., "arm64" (Apple Silicon) or "x86_64"
```

### 2. Checks for Apple Silicon (Metal)
```bash
if [[ "$OS" == "Darwin" && "$ARCH" == "arm64" ]]; then
    FEATURE="metal"
fi
```
- **Logic**: If Mac + ARM chip -> Use Metal!
- **Result**: Enables `features = ["metal"]`

### 3. Checks for NVIDIA (CUDA)
```bash
if command -v nvcc &> /dev/null; then
    FEATURE="cuda"
fi
```
- **Logic**: If `nvcc` (NVIDIA Compiler) exists -> Use CUDA!
- **Result**: Enables `features = ["cuda"]`

### 4. Checks for AMD (ROCm)
```bash
if command -v hipcc &> /dev/null; then
    FEATURE="rocm"
fi
```
- **Logic**: If `hipcc` (AMD Compiler) exists -> Use ROCm!
- **Result**: Enables `features = ["rocm"]`

### 5. Builds with the Correct Flag
```bash
cargo build --release --no-default-features --features "$FEATURE"
```
- **Magic**: It passes the *detected* feature to Cargo!
- **Outcome**: You get a binary optimized for *your* hardware.

---

## Why This is Powerful

1. **Portability**: You can git clone this repo to a Linux server with NVIDIA, run `./build.sh`, and it *just works*.
2. **Safety**: It prevents you from building CUDA on a Mac (which would fail).
3. **Simplicity**: You don't need to remember `cargo build --features ...`. Just run the script!

**It's your intelligent build assistant!** 🤖
