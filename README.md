# EXSA Engine

EXSA Engine is a production-grade local LLM inference server written in Rust, built on top of llama.cpp via `llama-cpp-2`.
It provides a **streaming-first OpenAI-compatible API**, **GGUF-native model lifecycle controls**, and **optional built-in RAG** (Postgres + optional Qdrant + embeddings) for retrieval-augmented chat.

This README documents the **engine only** so the `exsa-engine/` directory can be published as a standalone open-source repository.

---

## Why EXSA Engine (GGUF-first)

EXSA is designed around the GGUF ecosystem and the realities of running models locally:

- **GGUF compatibility**: load and serve any GGUF model supported by llama.cpp (architectures + quantizations supported upstream).
- **Streaming-first**: Server-Sent Events (SSE) by default for low latency and smooth UI integration.
- **Hardware-aware builds**: one build command that chooses the best backend (Metal/CUDA/Vulkan/CPU) for your machine.
- **Operational safety**: request queue, optional rate limiting, and safe model path rules for runtime model switching.
- **RAG that works in production**: ingestion + retrieval endpoints and per-request RAG injection with prompt-injection guardrails.

If you want the engine to “feel” fast, the defaults emphasize:

- stable concurrency (queue + backpressure)
- predictable latency (timeouts, bounded buffers)
- sane prompt construction (template selection + stop sequences)

---

## Feature summary

### Inference & API

- **OpenAI-style chat completions**: `POST /v1/chat/completions` (SSE streaming)
- **Embeddings endpoint**: `POST /v1/embeddings` (OpenAI-compatible request/response)
- **Legacy generation endpoint**: `POST /v1/generate` (SSE streaming)
- **Health & status**: `GET /v1/health`, `GET /v1/status`

### Model lifecycle (GGUF)

- **List models on disk**: `GET /v1/models/list`
- **Inspect active model**: `GET /v1/models/active`, `GET /v1/model/info`
- **Switch models at runtime**: `POST /v1/models/load` (restricted to models directory)
- **Reload active model**: `POST /v1/models/reload`

### Built-in RAG (optional)

- **Document ingestion** into Postgres (chunked, deduplicated)
- **Retrieval** via:
  - **Vector mode**: embeddings + Qdrant (Cosine distance)
  - **Lexical mode**: Postgres full-text search (no embeddings/Qdrant)
- **Per-request injection** into chat via `rag` options on `/v1/chat/completions`
- **RAG APIs**: status, ingest, list, delete, search

---

## Platform & backend support

Backends are compile-time selected through Cargo features, with `build-engine.sh` doing automatic detection:

- macOS Apple Silicon: **Metal**
- Linux / Windows (with CUDA toolkit): **CUDA** (detected via `nvcc`)
- Cross-platform: **Vulkan** (if available)
- Fallback: **CPU**

Notes:

- ROCm/HIP may be available in llama.cpp, but is currently not enabled in this crate’s feature set (dependency-version dependent).

---

## Requirements

### Build requirements

- Rust toolchain (`cargo`): https://rustup.rs/
- `cmake` (required to build llama.cpp)
- A C/C++ toolchain
  - macOS: Xcode Command Line Tools (`xcode-select --install`)
  - Linux: distro build tools (e.g. `build-essential`)

The build script fails fast with clear install instructions if `cmake` is missing.

### Runtime requirements

- A local **GGUF** model file

---

## Quick start

### 1) Build (recommended)

Automatic hardware detection:

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

### 3) Chat (OpenAI-compatible, streaming)

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

---

## Configuration (environment variables)

EXSA Engine is configured primarily via environment variables.

### Core inference

- `MODEL_PATH` (required): path to the GGUF model to load
- `GPU_LAYERS` (default: `0`): number of layers to offload to GPU (backend-dependent)
- `CONTEXT_SIZE` (default: `4096`)
- `BATCH_SIZE` (default: `CONTEXT_SIZE`)

### Server

- `HOST` (default: `127.0.0.1`): bind address (`0.0.0.0` enables LAN access)
- `PORT` (default: `3000`)
- `MAX_QUEUE_SIZE` (default: `100`)
- `ENABLE_CORS` (default: `false`)

### Rate limiting (optional)

- `ENABLE_RATE_LIMIT` (default: `false`)
- `RATE_LIMIT_MAX` (default: `60`)
- `RATE_LIMIT_WINDOW` (default: `60` seconds)

### Continuous batching (optional)

These toggles control server-side request batching behavior.

- `ENABLE_CONTINUOUS_BATCHING` (default: `false`)
- `MAX_BATCH_SIZE` (default: `8`)
- `BATCH_TIMEOUT_MS` (default: `100`)

---

## API specification

### Endpoints

| Endpoint | Method | Notes |
|---|---:|---|
| `/v1/health` | GET | Health + uptime + queue stats |
| `/v1/status` | GET | Lightweight server status |
| `/v1/model/info` | GET | Current model info |
| `/v1/generate` | POST | Streaming SSE token events |
| `/v1/chat/completions` | POST | OpenAI-style streaming chat completions |
| `/v1/embeddings` | POST | OpenAI-compatible embeddings endpoint |
| `/v1/models/list` | GET | Lists `.gguf` files under the models directory |
| `/v1/models/active` | GET | Active model metadata |
| `/v1/models/load` | POST | Switch model (GGUF only, within models dir) |
| `/v1/models/reload` | POST | Reload current model |
| `/v1/models/unload` | POST | Not supported |
| `/v1/rag/status` | GET | RAG status + active defaults |
| `/v1/rag/documents` | GET | List documents (`kb`, `limit`) |
| `/v1/rag/documents` | POST | Ingest a document (multipart) |
| `/v1/rag/documents/:id` | DELETE | Delete document (also deletes vectors if enabled) |
| `/v1/rag/search` | POST | Search chunks (lexical or vector mode) |

### Models directory rules (runtime switching)

For safety, model switching is restricted:

- Only `.gguf` models are accepted
- Model paths must be inside the resolved models directory:
  - set `MODELS_DIR`, or
  - use `./models` or `../models`
- Switching/reloading is rejected while requests are queued

---

## RAG (Retrieval-Augmented Generation)

EXSA Engine includes an **optional, built-in RAG service**. When enabled, it provides ingestion + retrieval endpoints and can inject retrieved context into `/v1/chat/completions`.

### What “built-in RAG” means here

- **Storage/metadata**: Postgres (documents + chunks + dedup)
- **Retrieval**:
  - **Vector mode**: embeddings + Qdrant
  - **Lexical mode**: Postgres full-text search only
- **Injection**: retrieved chunks are inserted as a `system` message marked as **UNTRUSTED** reference material (prompt-injection guardrail).

### Enable RAG (minimum)

RAG requires Postgres.

```bash
export EXSA_RAG_ENABLED=true
export EXSA_RAG_POSTGRES_URL="postgres://user:pass@127.0.0.1:5432/exsa"
```

If you also want vector search, enable Qdrant + an embeddings provider:

```bash
export EXSA_RAG_VECTOR_SEARCH_ENABLED=true
export EXSA_RAG_QDRANT_URL="http://127.0.0.1:6333"
export EXSA_RAG_EMBEDDINGS_URL="http://127.0.0.1:3000/v1/embeddings"
```

If you want **maximum stability on macOS/Metal**, consider running embeddings on a **separate CPU-only engine instance** (or set `EXSA_RAG_VECTOR_SEARCH_ENABLED=false` for lexical-only retrieval).

### RAG configuration

- `EXSA_RAG_ENABLED` (default: `false`)
- `EXSA_RAG_POSTGRES_URL` (required when enabled)
- `EXSA_RAG_VECTOR_SEARCH_ENABLED` (default: `true`)
- `EXSA_RAG_QDRANT_URL` (required when vector mode enabled)
- `EXSA_RAG_QDRANT_COLLECTION` (default: `exsa_rag_chunks`)
- `EXSA_RAG_EMBEDDINGS_URL` (required when vector mode enabled)
- `EXSA_RAG_EMBEDDINGS_MODEL` (optional)
- `EXSA_RAG_DEFAULT_KB` (default: `default`)
- `EXSA_RAG_CHUNK_MAX_CHARS` (default: `1400`)
- `EXSA_RAG_CHUNK_OVERLAP_CHARS` (default: `200`)
- `EXSA_RAG_RETRIEVE_TOP_K` (default: `6`)
- `EXSA_RAG_MAX_CONTEXT_CHARS` (default: `8000`)
- `EXSA_RAG_INIT_TIMEOUT_SECS` (default: `15`)
- `EXSA_RAG_PG_CONNECT_TIMEOUT_SECS` (default: `5`)
- `EXSA_RAG_PG_ACQUIRE_TIMEOUT_SECS` (default: `5`)
- `EXSA_RAG_HTTP_TIMEOUT_SECS` (default: `10`)

### Ingest a document

The ingest endpoint accepts multipart form data with either `file` or `text`.

```bash
curl -X POST "http://127.0.0.1:3000/v1/rag/documents?kb=default&title=MyDoc" \
  -F "file=@./README.md" \
  -F "source_name=README.md"
```

### Search

```bash
curl -X POST http://127.0.0.1:3000/v1/rag/search \
  -H "Content-Type: application/json" \
  -d '{"query":"what is the build command?","kb":"default","top_k":6}'
```

### Use RAG in chat completions

Add a `rag` object to the request body; the engine uses the last user message as the retrieval query.

```bash
curl -N -X POST http://127.0.0.1:3000/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{
    "model": "local-model",
    "messages": [{"role":"user","content":"Summarize how to build the engine."}],
    "stream": true,
    "rag": {"enabled": true, "kb": "default", "top_k": 6}
  }'
```

---

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

For details on the build system, see `BUILD_SYSTEM.md`.

---

## Performance & benchmarking

Performance depends on model architecture, quantization, context size, GPU layers, and hardware.

- Performance notes and benchmarks: `docs/PERFORMANCE_METRICS.md`
- Build + backend analysis: `docs/BUILD_SCRIPT_EXPLAINED.md`
- Competitive notes: `docs/COMPETITIVE_ANALYSIS.md`
- Raw logs: `PERFORMANCE_TEST_LOG.md`

The repository also includes a `benchmark` binary for local testing.

---

## Security & operational notes

- Default bind is **localhost** (`127.0.0.1`) and **CORS is off**.
- Model switching is restricted to a models directory to prevent arbitrary file access.
- Retrieved RAG text is treated as **untrusted** and injected with explicit “do not follow instructions” guardrails.

---

## Troubleshooting

### Build fails due to missing CMake

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

### My OpenAI client doesn’t work

The engine is **streaming-first**. If your client expects a fully-buffered response or strict OpenAI streaming sentinels, you may need a small adapter/proxy.

---

## License

MIT License. See `LICENSE`.
