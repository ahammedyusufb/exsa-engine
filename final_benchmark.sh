#!/bin/bash
# FINAL EXSA Engine Performance Test - Accurate Measurements
# Uses streaming API with manual token counting and log analysis

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL_PATH="$SCRIPT_DIR/src/model/LFM2-2.6B-Q4_K_M.gguf"
ENGINE_BIN="$SCRIPT_DIR/target/release/exsa-engine"
PORT=3000
API_URL="http://127.0.0.1:$PORT"
LOG_FILE="/tmp/exsa-final-benchmark.log"

echo "========================================="
echo "EXSA ENGINE - FINAL PERFORMANCE TEST"
echo "NO MERCY - ACCURATE MEASUREMENTS"
echo "========================================="
echo ""

# Kill any existing engine
pkill -f exsa-engine || true
sleep 2

# Verify model
MODEL_SIZE=$(ls -lh "$MODEL_PATH" | awk '{print $5}')
echo "Model: LFM2-2.6B Q4_K_M ($MODEL_SIZE)"
echo ""

# Measure cold startup
echo "========================================="
echo "TEST 1: COLD STARTUP TIME"
echo "========================================="

START=$(date +%s.%N)

MODEL_PATH="$MODEL_PATH" \
GPU_LAYERS=99 \
RUST_LOG=info \
$ENGINE_BIN > "$LOG_FILE" 2>&1 &

ENGINE_PID=$!

# Wait for ready
MAX_WAIT=60
for i in $(seq 1 $MAX_WAIT); do
    if curl -s "$API_URL/v1/health" >/dev/null 2>&1; then
        END=$(date +%s.%N)
        STARTUP=$(echo "$END - $START" | bc)
        printf "\n✅ Engine Ready\n🚀 COLD STARTUP: %.3fs\n\n" "$STARTUP"
        break
    fi
    sleep 0.5
done

sleep 2

# Run multiple tests with streaming
echo "========================================="
echo "TEST 2: TOKEN GENERATION SPEED"  
echo "========================================="
echo ""

run_test() {
    local name="$1"
    local prompt="$2"
    local max_tokens="$3"
    
    echo "Test: $name"
    echo "Prompt: $prompt"
    echo ""
    
    local start=$(date +%s.%N)
    local tokens=0
    local first_token_time=""
    local first_content=""
    local full_output=""
    
    # Stream and count tokens
    while IFS= read -r line; do
        # Check for first token
        if [ -z "$first_token_time" ] && echo "$line" | grep -q '"content"'; then
            first_token_time=$(date +%s.%N)
            local ttft=$(echo "$first_token_time - $start" | bc)
            printf "⚡ First Token: %.3fs\n" "$ttft"
        fi
        
        # Count content tokens (each chunk in delta is roughly a token)
        if echo "$line" | grep -q '"content":'; then
            tokens=$((tokens + 1))
            # Extract content (remove SSE prefix and parse JSON)
            content=$(echo "$line" | sed 's/^data: //' | jq -r '.choices[0].delta.content // empty' 2>/dev/null || echo "")
            if [ -n "$content" ]; then
                full_output="${full_output}${content}"
            fi
        fi
        
        # Check for completion
        if echo "$line" | grep -q '"finish_reason":"stop"'; then
            break
        fi
    done < <(curl -s -X POST "$API_URL/v1/chat/completions" \
        -H "Content-Type: application/json" \
        -d "{\"model\":\"LFM2-2.6B\",\"messages\":[{\"role\":\"user\",\"content\":\"$prompt\"}],\"max_tokens\":$max_tokens,\"stream\":true,\"temperature\":0.7}")
    
    local end=$(date +%s.%N)
    local total=$(echo "$end - $start" | bc)
    local tps=$(echo "scale=2; $tokens / $total" | bc)
    
    # Display response preview
    echo "Response: ${full_output:0:150}..."
    printf "📊 Tokens: ~%d, Time: %.3fs, Speed: %.2f t/s\n\n" "$tokens" "$total" "$tps"
    
    # Return TPS for averaging
    echo "$tps"
}

# Test 1: Simple
tps1=$(run_test "Simple Math" "Count from 1 to 10" 50)

# Test 2: Medium
tps2=$(run_test "Medium Complexity" "Explain what machine learning is in 2 sentences" 100)

#Test 3: Hard Prompt
tps3=$(run_test "Hard - ML Comparison" "Explain the difference between supervised and unsupervised machine learning with examples" 150)

# Test 4: Maximum length
tps4=$(run_test "Maximum Generation" "Write a detailed step-by-step guide on training a neural network" 250)

# Calculate average
avg_tps=$(echo "scale=2; ($tps1 + $tps2 + $tps3 + $tps4) / 4" | bc)

echo "========================================="
echo "TEST 3: STRESS TEST - LONG GENERATION"
echo "========================================="
echo ""

STRESS_START=$(date +%s.%N)
STRESS_TOKENS=0

while IFS= read -r line; do
    if echo "$line" | grep -q '"content":'; then
        STRESS_TOKENS=$((STRESS_TOKENS + 1))
    fi
    if echo "$line" | grep -q '"finish_reason":"stop"'; then
        break
    fi
done < <(curl -s -X POST "$API_URL/v1/chat/completions" \
    -H "Content-Type: application/json" \
    -d '{"model":"LFM2-2.6B","messages":[{"role":"user","content":"Provide a comprehensive explanation of deep learning, including neural network architectures, backpropagation, optimization algorithms, and practical applications. Be very detailed."}],"max_tokens":300,"stream":true,"temperature":0.8}')

STRESS_END=$(date +%s.%N)
STRESS_TIME=$(echo "$STRESS_END - $STRESS_START" | bc)
STRESS_TPS=$(echo "scale=2; $STRESS_TOKENS / $STRESS_TIME" | bc)

printf "Stress test: ~%d tokens in %.3fs = %.2f t/s\n\n" "$STRESS_TOKENS" "$STRESS_TIME" "$STRESS_TPS"

# Memory usage
MEM_KB=$(ps -o rss= -p $ENGINE_PID 2>/dev/null || echo "0")
MEM_MB=$(echo "scale=2; $MEM_KB / 1024" | bc)

# Analyze logs for GPU info
echo "========================================="
echo "FINAL PERFORMANCE REPORT"
echo "========================================="
echo ""
echo "📋 SYSTEM INFO"
echo "   Model: LFM2-2.6B Q4_K_M"
echo "   Size: $MODEL_SIZE"
printf "   Memory: %.2f MB\n" "$MEM_MB"
echo ""

# Check GPU from logs
if grep -q "Metal" "$LOG_FILE"; then
    echo "   GPU: ✅ Metal (Apple Silicon)"
else
    echo "   GPU: ❌ CPU only"
fi
echo ""

printf "⏱️  COLD STARTUP: %.3fs\n" "$STARTUP"
echo ""
echo "🔥 TOKEN GENERATION PERFORMANCE"
printf "   Test 1 (Simple):    %.2f t/s\n" "$tps1"
printf "   Test 2 (Medium):    %.2f t/s\n" "$tps2"
printf "   Test 3 (Hard):      %.2f t/s\n" "$tps3"
printf "   Test 4 (Max Gen):   %.2f t/s\n" "$tps4"
printf "   Stress Test (300t): %.2f t/s\n" "$STRESS_TPS"
echo ""
printf "   📊 AVERAGE: %.2f t/s\n" "$avg_tps"
echo ""
echo "========================================="
echo "✅ BENCHMARK COMPLETE - NO MERCY!"
echo "========================================="

# Kill engine
kill $ENGINE_PID 2>/dev/null || true

echo ""
echo "Full logs: $LOG_FILE"
