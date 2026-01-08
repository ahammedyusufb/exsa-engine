# Exsa-Engine - Production Ready

**Version**: 0.1.0  
**Status**: Production-Ready with OpenAI Compatibility  
**Last Updated**: November 24, 2025

## Quick Start

### Build (Automatic Hardware Detection)

The engine uses intelligent hardware detection to automatically select the optimal backend:

```bash
# Auto-detect hardware and build (recommended)
make build
# or
./build-engine.sh

# The build system will automatically detect:
# - ✅ Apple Silicon → Metal GPU
# - ✅ NVIDIA GPU → CUDA
# - ✅ AMD GPU → ROCm (when available)
# - ✅ Fallback → CPU-only
```

### Run

```bash
MODEL_PATH="models/your-model.gguf" \
GPU_LAYERS=99 \
./target/release/exsa-engine

# Health check
curl http://127.0.0.1:3000/v1/health

# OpenAI-compatible usage
curl -X POST http://127.0.0.1:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "your-model",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 100,
    "stream": true
  }'
```

### Manual Backend Override

```bash
# Force specific backend
make metal    # Apple Silicon Metal
make cuda     # NVIDIA CUDA
make vulkan   # Cross-platform Vulkan
make cpu      # CPU-only (no GPU)

# Or with environment variables
FORCE_METAL=1 ./build-engine.sh
FORCE_CUDA=1 ./build-engine.sh
FORCE_CPU=1 ./build-engine.sh
```

## Features

- ✅ **OpenAI-Compatible API** - Works with LangChain, AutoGen, SillyTavern
- ✅ **Prompt Templating** - ChatML, Llama3, Alpaca support
- ✅ **GPU Acceleration** - 100% Metal/CUDA/ROCm support
- ✅ **High Performance** - 37-61 t/s (avg ~51 t/s)
- ✅ **Fast Startup** - Sub-second model loading
- ✅ **Production Ready** - Clean code, 0 warnings

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/health` | GET | Health check |
| `/v1/status` | GET | Server status |
| `/v1/generate` | POST | Legacy generation endpoint |
| `/v1/chat/completions` | POST | OpenAI-compatible chat |

## Documentation

- `docs/` - Performance metrics, architecture docs
- `TECH_STACK.md` - Complete technical documentation
- `CODE_CHANGES_VERIFICATION.md` - Recent updates

## License

MIT License - See LICENSE file

---

*Built with Rust, llama.cpp, and Metal GPU acceleration*
