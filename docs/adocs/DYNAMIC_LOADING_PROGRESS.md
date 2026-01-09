# Dynamic Model Loading - Implementation Progress

## Feature #6: Hot-Swap Model Loading

**Status**: IN PROGRESS

### Step 1: Model Manager Core ✅

**Created**: `src/model/manager.rs` (220 lines)

**Features**:
- `ModelManager` class with:
  - Model caching (multiple models in memory)
  - Hot-swapping capability
  - Model metadata tracking
  - LRU cache management (prepared)
  - Thread-safe (Arc + RwLock)

**Key Methods**:
1. `new()` - Initialize with default model
2. `switch_model()` - Hot-swap to cached model
3. `load_model()` - Load new model into cache
4. `list_models()` - Get all available models
5. `get_model_info()` - Get model metadata
6. `unload_model()` - Remove from cache

### Step 2: Integration (TODO)

**Need to**:
1. Update InferenceEngine to use ModelManager
2. Add model switching API endpoint
3. Update /v1/models endpoint to list cached models
4. Add model loading endpoint

### Step 3: Configuration (TODO)

**Add to .env**:
```
# Model configuration
DEFAULT_MODEL_NAME=LFM2-2.6B
MODEL_CACHE_SIZE=3
MODELS_DIR=./models
```

### Example Usage

```rust
// Initialize
let manager = ModelManager::new(
    "LFM2".to_string(),
    PathBuf::from("models/lfm2.gguf"),
    config,
    backend,
    3  // cache size
)?;

// Load another model
manager.load_model(
    "Mistral".to_string(),
    PathBuf::from("models/mistral.gguf"),
    mistral_config
)?;

// Hot-swap
manager.switch_model("Mistral")?;  // Instant!

// Get active model
let model = manager.get_active_model()?;
```

---

**Current**: Model manager core complete, ready for integration
