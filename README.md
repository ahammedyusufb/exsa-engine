# EXSA Engine

EXSA Engine is a local LLM inference server written in Rust, built on top of llama.cpp via `llama-cpp-2`. It’s designed to be fast, configurable, and easy to integrate (OpenAI-style chat completions, streaming-first), while keeping a secure default posture (localhost-only, CORS off).

## What you get

- **Local inference API**: HTTP server with streaming responses (SSE)
- **OpenAI-style chat completions**: `/v1/chat/completions` for ecosystem compatibility
- **Prompt templating**: model-aware templates (e.g. ChatML/Llama-style) + template stop-sequences
- **Hardware-aware builds**: auto-select backend (Metal/CUDA/Vulkan/CPU) from a single build command
- **Operational controls**: request queue, health/status endpoints, optional rate limiting
- **Model lifecycle**: list models on disk and switch models at runtime (GGUF only)

## Platform & backend support

Backends are compile-time selected through Cargo features and the build script:

- **macOS Apple Silicon**: Metal
- **Linux / Windows (with CUDA toolkit)**: CUDA (detected via `nvcc`)
- **Cross-platform**: Vulkan (if available)
- **Fallback**: CPU

Note: ROCm/HIP is currently not enabled in this crate’s Cargo features (dependency support is version-dependent).

## Requirements

### Build requirements

- Rust toolchain (`cargo`): https://rustup.rs/
- **CMake** (required by llama.cpp native build)
- A C/C++ toolchain
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Linux: `build-essential` / gcc + make (varies by distro)

The build script will fail fast with clear instructions if `cmake` is missing.

### Runtime requirements

- A local **GGUF** model file

## Quick start

### 1) Build

Automatic hardware detection (recommended):

```bash
./build-engine.sh
```

Or via Makefile:

```bash
make build
```

### 2) Run

`MODEL_PATH` is required.

```bash
MODEL_PATH="models/your-model.gguf" \
GPU_LAYERS=0 \
./target/release/exsa-engine
```

Health check:

```bash
curl http://127.0.0.1:3000/v1/health
```

### 3) Send a chat request (streaming)

```bash
curl -N -X POST http://127.0.0.1:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "local-model",
    "messages": [{"role": "user", "content": "Hello!"}],
    "max_tokens": 128,
    "stream": true
  }'
```

This endpoint returns **SSE** events with JSON chunks (`chat.completion.chunk`). A final chunk is emitted with `finish_reason: "stop"`.

## Configuration

EXSA Engine is configured mainly via environment variables.

### Core

- `MODEL_PATH` (required): path to the GGUF model to load on startup
- `GPU_LAYERS` (default: `0`): number of layers to place on GPU (backend dependent)
- `CONTEXT_SIZE` (default: `4096`)
- `BATCH_SIZE` (default: `CONTEXT_SIZE`)

### Server

- `HOST` (default: `127.0.0.1`): bind address (`0.0.0.0` enables LAN access)
- `PORT` (default: `3000`)
- `ENABLE_CORS` (default: `false`)

### Rate limiting (optional)

- `ENABLE_RATE_LIMIT` (default: `false`)
- `RATE_LIMIT_MAX` (default: `60`)
- `RATE_LIMIT_WINDOW` (default: `60` seconds)

### Queueing

- `MAX_QUEUE_SIZE` (default: `100`)

### Continuous batching (optional)

- `ENABLE_CONTINUOUS_BATCHING` (default: `false`)
- `MAX_BATCH_SIZE` (default: `8`)
- `BATCH_TIMEOUT_MS` (default: `100`)

## API

### Endpoints

| Endpoint | Method | Notes |
|---|---:|---|
| `/v1/health` | GET | Health + uptime + queue stats |
| `/v1/status` | GET | Lightweight server status |
| `/v1/model/info` | GET | Current model info |
| `/v1/generate` | POST | Streaming SSE token events (legacy-style) |
| `/v1/chat/completions` | POST | OpenAI-style streaming chat completions |
| `/v1/models/list` | GET | Lists `.gguf` files under the models directory |
| `/v1/models/active` | GET | Active model metadata |
| `/v1/models/load` | POST | Switch model (only `.gguf`, only inside models dir) |
| `/v1/models/reload` | POST | Reload current model |
| `/v1/models/unload` | POST | Currently not supported |

### Models directory rules

For safety, model switching is restricted:

- Only `.gguf` models are accepted
- Model paths must be inside the resolved models directory
  - set `MODELS_DIR`, or
  - use `./models` or `../models`
- Model switching/reloading is rejected while requests are queued

## Build options

### Force a backend

```bash
FORCE_METAL=1 ./build-engine.sh
FORCE_CUDA=1 ./build-engine.sh
FORCE_CPU=1 ./build-engine.sh
```

Or via Makefile:

```bash
make metal
make cuda
make vulkan
make cpu
```

### Debug build

```bash
BUILD_TYPE=debug ./build-engine.sh
```

## Performance & footprint

Performance depends heavily on the model, quantization, context size, GPU layers, and hardware.

- This repository includes performance notes and logs under:
  - `docs/`
  - `PERFORMANCE_TEST_LOG.md`

Binary size also depends on target/backends. To check your local build:

```bash
ls -lh target/release/exsa-engine
```

## RAG (Retrieval-Augmented Generation)

EXSA Engine focuses on fast local inference and an API surface that is easy to plug into other tools.

- It **does not ship a built-in vector database** or a complete “RAG pipeline” inside the engine.
- You can implement RAG in your app (LangChain/LlamaIndex/etc.) and point it to EXSA Engine’s OpenAI-style endpoint.

## Troubleshooting

### Build fails complaining about CMake

Install `cmake` and retry. On macOS:

```bash
xcode-select --install
brew install cmake
```

### Server exits saying MODEL_PATH must be set

Provide a GGUF model path:

```bash
MODEL_PATH="models/your-model.gguf" ./target/release/exsa-engine
```

### “OpenAI compatible” but my client doesn’t work

The server is **streaming-first**. Some OpenAI clients expect non-streaming responses or a `[DONE]` sentinel.

- Use streaming (`stream: true`) when possible.
- If your client requires strict OpenAI semantics, you may need a small adapter/proxy.

## License

MIT License. See `LICENSE`.
