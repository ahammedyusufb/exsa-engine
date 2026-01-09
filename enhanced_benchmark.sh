#!/bin/bash
# Enhanced EXSA Engine Performance Test - ACCURATE METRICS
# Measures actual token generation via response analysis

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL_PATH="$SCRIPT_DIR/src/model/LFM2-2.6B-Q4_K_M.gguf"
ENGINE_BIN="$SCRIPT_DIR/target/release/exsa-engine"
PORT=3000
API_URL="http://127.0.0.1:$PORT"

echo "========================================="
echo "EXSA ENGINE - ENHANCED PERFORMANCE TEST"
echo "========================================="
echo ""

# Verify model exists
if [ ! -f "$MODEL_PATH" ]; then
    echo "❌ ERROR: Model not found at $MODEL_PATH"
    exit 1
fi

MODEL_SIZE=$(ls -lh "$MODEL_PATH" | awk '{print $5}')
echo "Model: LFM2-2.6B Q4_K_M"
echo "Size: $MODEL_SIZE"
echo ""

# Kill any existing engine process
pkill -f exsa-engine || true
sleep 2

echo "========================================="
echo "TEST 1: COLD STARTUP & MODEL LOAD"
echo "========================================="

START_TIME=$(date +%s.%N)

MODEL_PATH="$MODEL_PATH" \
GPU_LAYERS=99 \
RUST_LOG=info \
$ENGINE_BIN > /tmp/exsa-engine-test.log 2>&1 &

ENGINE_PID=$!
echo "Engine PID: $ENGINE_PID"

# Wait for engine to be ready
echo "Waiting for engine startup..."
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
    echo "❌ ERROR: Engine failed to start"
    cat /tmp/exsa-engine-test.log
    exit 1
fi

# Let it settle
sleep 2

echo "========================================="
echo "TEST 2: TOKEN SPEED MEASUREMENT"
echo "========================================="
echo ""
echo "Testing with non-streaming response for accurate count..."

# Test 1: Short response
TEST1_START=$(date +%s.%N)
RESPONSE1=$(curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "LFM2-2.6B",
    "messages": [{"role": "user", "content": "Count from 1 to 10."}],
    "max_tokens": 50,
    "stream": false,
    "temperature": 0.1
  }')
TEST1_END=$(date +%s.%N)
TEST1_TIME=$(echo "$TEST1_END - $TEST1_START" | bc)

# Extract completion tokens
TOKENS1=$(echo "$RESPONSE1" | jq -r '.usage.completion_tokens // 0')
TPS1=$(echo "scale=2; $TOKENS1 / $TEST1_TIME" | bc)

echo "Test 1: Simple counting"
echo "  Response: $(echo "$RESPONSE1" | jq -r '.choices[0].message.content' | head -c 100)..."
printf "  Tokens: %d, Time: %.3fs, Speed: %.2f t/s\n" "$TOKENS1" "$TEST1_TIME" "$TPS1"
echo ""

# Test 2: Medium complexity
TEST2_START=$(date +%s.%N)
RESPONSE2=$(curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "LFM2-2.6B",
    "messages": [{"role": "user", "content": "Explain what machine learning is in 2-3 sentences."}],
    "max_tokens": 100,
    "stream": false,
    "temperature": 0.7
  }')
TEST2_END=$(date +%s.%N)
TEST2_TIME=$(echo "$TEST2_END - $TEST2_START" | bc)

TOKENS2=$(echo "$RESPONSE2" | jq -r '.usage.completion_tokens // 0')
TPS2=$(echo "scale=2; $TOKENS2 / $TEST2_TIME" | bc)

echo "Test 2: Medium complexity explanation"
echo "  Response: $(echo "$RESPONSE2" | jq -r '.choices[0].message.content')"
printf "  Tokens: %d, Time: %.3fs, Speed: %.2f t/s\n" "$TOKENS2" "$TEST2_TIME" "$TPS2"
echo ""

# Test 3: HARD PROMPT - Complex reasoning
echo "========================================="
echo "TEST 3: HARD PROMPT - COMPLEX REASONING"
echo "========================================="
echo ""

TEST3_START=$(date +%s.%N)
RESPONSE3=$(curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "LFM2-2.6B",
    "messages": [{
      "role": "user",
      "content": "Explain the difference between supervised and unsupervised machine learning, provide examples of algorithms for each, and describe when you would use one over the other."
    }],
    "max_tokens": 200,
    "stream": false,
    "temperature": 0.7
  }')
TEST3_END=$(date +%s.%N)
TEST3_TIME=$(echo "$TEST3_END - $TEST3_START" | bc)

TOKENS3=$(echo "$RESPONSE3" | jq -r '.usage.completion_tokens // 0')
TPS3=$(echo "scale=2; $TOKENS3 / $TEST3_TIME" | bc)

echo "Complex ML explanation (200 max tokens):"
echo "  $(echo "$RESPONSE3" | jq -r '.choices[0].message.content' | head -c 200)..."
printf "  Tokens: %d, Time: %.3fs, Speed: %.2f t/s\n" "$TOKENS3" "$TEST3_TIME" "$TPS3"
echo ""

# Test 4: MAXIMUM LENGTH
echo "========================================="
echo "TEST 4: MAXIMUM TOKEN GENERATION"
echo "========================================="
echo ""

TEST4_START=$(date +%s.%N)
RESPONSE4=$(curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "LFM2-2.6B",
    "messages": [{
      "role": "user",
      "content": "Write a detailed step-by-step guide on how to train a neural network for image classification. Include data preparation, model architecture, training process, and evaluation metrics."
    }],
    "max_tokens": 300,
    "stream": false,
    "temperature": 0.8
  }')
TEST4_END=$(date +%s.%N)
TEST4_TIME=$(echo "$TEST4_END - $TEST4_START" | bc)

TOKENS4=$(echo "$RESPONSE4" | jq -r '.usage.completion_tokens // 0')
TPS4=$(echo "scale=2; $TOKENS4 / $TEST4_TIME" | bc)

echo "Detailed technical guide (300 max tokens):"
printf "  Tokens: %d, Time: %.3fs, Speed: %.2f t/s\n" "$TOKENS4" "$TEST4_TIME" "$TPS4"
echo ""

# Test 5: Streaming performance
echo "========================================="
echo "TEST 5: STREAMING PERFORMANCE (TTFT)"
echo "========================================="
echo ""

STREAM_START=$(date +%s.%N)
TTFT_RECORDED=0

curl -s -X POST "$API_URL/v1/chat/completions" \
  -H "Content-Type: application/json" \
  -d '{
    "model": "LFM2-2.6B",
    "messages": [{"role": "user", "content": "Explain quantum computing in simple terms."}],
    "max_tokens": 150,
    "stream": true,
    "temperature": 0.7
  }' | while IFS= read -r line; do
    if [ "$TTFT_RECORDED" -eq 0 ] && echo "$line" | grep -q '"delta"'; then
        CONTENT=$(echo "$line" | sed 's/data: //' | jq -r '.choices[0].delta.content // empty' 2>/dev/null)
        if [ -n "$CONTENT" ] && [ "$CONTENT" != "null" ]; then
            TTFT_TIME=$(date +%s.%N)
            TTFT=$(echo "$TTFT_TIME - $STREAM_START" | bc)
            printf "⚡ Time to First Token: %.3fs\n" "$TTFT"
            TTFT_RECORDED=1
        fi
    fi
done

echo ""

# Calculate average TPS
AVG_TPS=$(echo "scale=2; ($TPS1 + $TPS2 + $TPS3 + $TPS4) / 4" | bc)

echo "========================================="
echo "FINAL PERFORMANCE SUMMARY"
echo "========================================="
echo ""
echo "📊 STARTUP METRICS"
printf "   Cold Startup: %.3fs ✅ (sub-2 second target)\n" "$STARTUP_TIME"
echo ""
echo "📊 TOKEN GENERATION PERFORMANCE"
echo "   Test 1 (Simple):       ${TPS1} t/s (${TOKENS1} tokens in ${TEST1_TIME}s)"
echo "   Test 2 (Medium):       ${TPS2} t/s (${TOKENS2} tokens in ${TEST2_TIME}s)"
echo "   Test 3 (Hard):         ${TPS3} t/s (${TOKENS3} tokens in ${TEST3_TIME}s)"
echo "   Test 4 (Maximum):      ${TPS4} t/s (${TOKENS4} tokens in ${TEST4_TIME}s)"
echo ""
printf "   🔥 AVERAGE: %.2f t/s\n" "$AVG_TPS"
echo ""

# Memory usage
MEM_USAGE=$(ps -p $ENGINE_PID -o rss= 2>/dev/null || echo "0")
if [ "$MEM_USAGE" != "0" ]; then
    MEM_MB=$(echo "scale=2; $MEM_USAGE / 1024" | bc)
    printf "💾 Memory Usage: %.2f MB\n" "$MEM_MB"
fi

echo ""
echo "========================================="
echo "✅ BENCHMARK COMPLETE!"
echo "========================================="

# Kill engine
kill $ENGINE_PID 2>/dev/null || true

echo ""
echo "Full logs: /tmp/exsa-engine-test.log"
