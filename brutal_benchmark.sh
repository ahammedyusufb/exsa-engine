#!/bin/bash
# EXSA Engine Performance Test - FIXED TOKEN COUNTING
# Accurate measurements via engine logs

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL_PATH="$SCRIPT_DIR/src/model/LFM2-2.6B-Q4_K_M.gguf"
ENGINE_BIN="$SCRIPT_DIR/target/release/exsa-engine"
PORT=3000
API_URL="http://127.0.0.1:$PORT"
LOG_FILE="/tmp/exsa-benchmark-$(date +%s).log"

echo "========================================="
echo "EXSA ENGINE - PERFORMANCE BENCHMARK"
echo "FIXED: Accurate token counting from logs"
echo "========================================="
echo ""
echo "Model: $MODEL_PATH"
echo "Binary: $ENGINE_BIN"
echo "Log: $LOG_FILE"
echo ""

# Verify model exists
if [ ! -f "$MODEL_PATH" ]; then
    echo "❌ ERROR: Model not found at $MODEL_PATH"
    exit 1
fi

MODEL_SIZE=$(ls -lh "$MODEL_PATH" | awk '{print $5}')
echo "Model Size: $MODEL_SIZE"
echo ""

# Kill any existing engine process
pkill -f exsa-engine || true
sleep 2

echo "========================================="
echo "TEST 1: COLD STARTUP & MODEL LOAD TIME"
echo "========================================="

START_TIME=$(date +%s.%N)

MODEL_PATH="$MODEL_PATH" \
GPU_LAYERS=99 \
RUST_LOG=info \
$ENGINE_BIN > "$LOG_FILE" 2>&1 &

ENGINE_PID=$!
echo "Engine PID: $ENGINE_PID"

# Wait for engine to be ready
echo "Waiting for engine to start..."
MAX_WAIT=60
COUNTER=0
while [ $COUNTER -lt $MAX_WAIT ]; do
    if curl -s "$API_URL/v1/health" > /dev/null 2>&1; then
        END_TIME=$(date +%s.%N)
        STARTUP_TIME=$(echo "$END_TIME - $START_TIME" | bc)
        echo ""
        echo "✅ Engine ready!"
        printf "🚀 COLD STARTUP TIME: %.3fs\n" "$STARTUP_TIME"
        echo ""
        break
    fi
    sleep 0.5
    COUNTER=$((COUNTER + 1))
done

if [ $COUNTER -eq $MAX_WAIT ]; then
    echo "❌ ERROR: Engine failed to start within ${MAX_WAIT} seconds"
    cat "$LOG_FILE"
    exit 1
fi

sleep 2

# Function to get last token count from log
get_last_token_count() {
    grep "Generation complete" "$LOG_FILE" | tail -1 | grep -oE "[0-9]+ tokens" | grep -oE "[0-9]+" || echo "0"
}

echo "========================================="
echo "TEST 2: SIMPLE WARMUP REQUEST"
echo "========================================="

WARMUP_START=$(date +%s.%N)
curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "LFM2-2.6B",
    "messages": [{"role": "user", "content": "Hi"}],
    "max_tokens": 10,
    "stream": false
  }' > /dev/null
WARMUP_END=$(date +%s.%N)
WARMUP_TIME=$(echo "$WARMUP_END - $WARMUP_START" | bc)
sleep 0.2
WARMUP_TOKENS=$(get_last_token_count)

printf "⏱️  Warmup time: %.3fs (%d tokens)\n\n" "$WARMUP_TIME" "$WARMUP_TOKENS"

echo "========================================="
echo "TEST 3: HARD PROMPT - COMPLEX REASONING"
echo "========================================="

HARD_PROMPT="Explain the difference between supervised and unsupervised machine learning, provide examples of algorithms for each, and describe when you would use one over the other."

echo "Prompt: $HARD_PROMPT"
echo ""
echo "Starting generation..."

TEST_START=$(date +%s.%N)

# Make request  
curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"LFM2-2.6B\",
    \"messages\": [{\"role\": \"user\", \"content\": \"$HARD_PROMPT\"}],
    \"max_tokens\": 200,
    \"stream\": false,
    \"temperature\": 0.7
  }" > /dev/null

TEST_END=$(date +%s.%N)
TOTAL_TIME=$(echo "$TEST_END - $TEST_START" | bc)

# Get token count from log
sleep 0.3
TOKENS=$(get_last_token_count)
TPS=$(echo "scale=2; $TOKENS / $TOTAL_TIME" | bc)

printf "\n📊 HARD PROMPT RESULTS:\n"
printf "   Total Time: %.3fs\n" "$TOTAL_TIME"
printf "   Tokens Generated: %d\n" "$TOKENS"
printf "   🔥 Tokens/Second: %.2f t/s\n\n" "$TPS"

echo "========================================="
echo "TEST 4: SPEED TEST - SHORT BURSTS"
echo "========================================="

for i in {1..3}; do
    echo "Burst #$i..."
    BURST_START=$(date +%s.%N)
    
    curl -s -X POST "$API_URL/v1/chat/completions" \
      -H "Content-Type: application/json" \
      -d '{
        "model": "LFM2-2.6B",
        "messages": [{"role": "user", "content": "Write a haiku about technology."}],
        "max_tokens": 50,
        "stream": false
      }' > /dev/null
    
    BURST_END=$(date +%s.%N)
    BURST_TIME=$(echo "$BURST_END - $BURST_START" | bc)
    sleep 0.2
    BURST_TOKENS=$(get_last_token_count)
    BURST_TPS=$(echo "scale=2; $BURST_TOKENS / $BURST_TIME" | bc)
    
    printf "   Burst #%d: %.3fs, %d tokens, %.2f t/s\n" "$i" "$BURST_TIME" "$BURST_TOKENS" "$BURST_TPS"
done

echo ""

echo "========================================="
echo "TEST 5: LONG CONTEXT STRESS TEST"
echo "========================================="

LONG_PROMPT="Write a comprehensive technical guide explaining neural networks, including architecture design, backpropagation, gradient descent optimization, regularization techniques, and practical tips."

echo "Prompt: $LONG_PROMPT"
echo "Starting long context generation..."

LONG_START=$(date +%s.%N)

curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d "{
    \"model\": \"LFM2-2.6B\",
    \"messages\": [{\"role\": \"user\", \"content\": \"$LONG_PROMPT\"}],
    \"max_tokens\": 300,
    \"stream\": false,
    \"temperature\": 0.8
  }" > /dev/null

LONG_END=$(date +%s.%N)
LONG_TIME=$(echo "$LONG_END - $LONG_START" | bc)
sleep 0.3
LONG_TOKENS=$(get_last_token_count)
LONG_TPS=$(echo "scale=2; $LONG_TOKENS / $LONG_TIME" | bc)

echo ""
printf "📊 LONG CONTEXT RESULTS:\n"
printf "   Total Time: %.3fs\n" "$LONG_TIME"
printf "   Tokens Generated: %d\n" "$LONG_TOKENS"
printf "   🔥 Tokens/Second: %.2f t/s\n\n" "$LONG_TPS"

# Memory usage
MEM_KB=$(ps -o rss= -p $ENGINE_PID 2>/dev/null || echo "0")
MEM_MB=$(echo "scale=2; $MEM_KB / 1024" | bc)

# Calculate average TPS
AVG_TPS=$(echo "scale=2; ($TPS + $LONG_TPS) / 2" | bc)

echo "========================================="
echo "FINAL PERFORMANCE SUMMARY"
echo "========================================="
echo "Model: $MODEL_PATH ($MODEL_SIZE)"
echo ""

# Check GPU from logs
if grep -q "Metal" "$LOG_FILE"; then
    GPU_NAME=$(grep "GPU name:" "$LOG_FILE" | head -1 | awk -F: '{print $2}' | xargs || printf "Apple Silicon")
    echo "GPU: ✅ $GPU_NAME (Metal)"
else
    echo "GPU: ❌ CPU only"
fi

echo ""
printf "⏱️  Cold Startup Time:    %.3fs\n" "$STARTUP_TIME"
printf "⏱️  Warmup Request:       %.3fs\n" "$WARMUP_TIME"
echo ""
echo "🔥 TOKEN GENERATION PERFORMANCE"
printf "   Hard Prompt:    %.2f t/s (%d tokens in %.2fs)\n" "$TPS" "$TOKENS" "$TOTAL_TIME"
printf "   Long Context:   %.2f t/s (%d tokens in %.2fs)\n" "$LONG_TPS" "$LONG_TOKENS" "$LONG_TIME"
printf "   AVERAGE:        %.2f t/s\n" "$AVG_TPS"
echo ""
printf "💾 Memory Usage: %.2f MB\n" "$MEM_MB"
echo ""
echo "========================================="
echo "✅ ALL TESTS COMPLETED!"
echo "========================================="

# Kill engine
kill $ENGINE_PID 2>/dev/null || true

echo ""
echo "Full logs saved to: $LOG_FILE"
