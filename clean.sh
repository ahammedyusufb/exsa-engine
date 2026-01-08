#!/usr/bin/env bash
# EXSA Engine - Cleanup Script
# Removes old build artifacts, caches, and temporary files

set -e

echo "========================================="
echo "EXSA ENGINE - CLEANUP SCRIPT"
echo "========================================="
echo ""

# Track total space freed
INITIAL_SIZE=0
FINAL_SIZE=0

# Function to get directory size
get_size() {
    if [[ -d "$1" ]]; then
        du -sh "$1" 2>/dev/null | awk '{print $1}' || echo "0"
    else
        echo "0"
    fi
}

# Function to remove directory and report
remove_dir() {
    local dir="$1"
    local description="$2"
    
    if [[ -d "$dir" ]]; then
        local size=$(get_size "$dir")
        echo "🗑️  Removing $description: $dir ($size)"
        rm -rf "$dir"
        echo "   ✅ Removed"
    else
        echo "⏭️  Skipping $description: $dir (not found)"
    fi
}

# Function to remove file and report
remove_file() {
    local file="$1"
    local description="$2"
    
    if [[ -f "$file" ]]; then
        local size=$(ls -lh "$file" 2>/dev/null | awk '{print $5}')
        echo "🗑️  Removing $description: $file ($size)"
        rm -f "$file"
        echo "   ✅ Removed"
    fi
}

echo "Starting cleanup..."
echo ""

# ============================================
# 1. Cargo Build Artifacts
# ============================================

echo "📦 CARGO BUILD ARTIFACTS"
echo "----------------------------------------"

# Main target directory
if [[ -d "target" ]]; then
    TARGET_SIZE=$(get_size "target")
    echo "🗑️  Removing target/ directory: $TARGET_SIZE"
    cargo clean
    echo "   ✅ Cargo clean completed"
else
    echo "⏭️  No target/ directory found"
fi

echo ""

# ============================================
# 2. Cargo Cache (Optional - Aggressive)
# ============================================

if [[ "$1" == "--aggressive" ]] || [[ "$1" == "-a" ]]; then
    echo "🔥 AGGRESSIVE MODE - CARGO CACHE"
    echo "----------------------------------------"
    
    CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
    
    # Registry cache
    remove_dir "$CARGO_HOME/registry/cache" "Cargo registry cache"
    
    # Git checkouts
    remove_dir "$CARGO_HOME/git/checkouts" "Cargo git checkouts"
    
    echo ""
fi

# ============================================
# 3. Temporary Build Files
# ============================================

echo "🧹 TEMPORARY BUILD FILES"
echo "----------------------------------------"

# Remove temporary logs
remove_file "/tmp/exsa-engine.log" "Engine log"
remove_file "/tmp/exsa-engine-test.log" "Test log"
remove_file "/tmp/fresh_test.log" "Fresh test log"
remove_file "/tmp/performance_test.log" "Performance test log"
remove_file "/tmp/test.log" "Generic test log"
remove_file "/tmp/build_output.log" "Build output log"
remove_file "/tmp/startup_begin.time" "Startup timing file"

# Remove any exsa-*.log files in /tmp
echo "🗑️  Removing /tmp/exsa-*.log files"
rm -f /tmp/exsa-*.log 2>/dev/null && echo "   ✅ Removed" || echo "   ⏭️  None found"

echo ""

# ============================================
# 4. Rust Incremental Compilation Cache
# ============================================

echo "🔧 INCREMENTAL COMPILATION CACHE"
echo "----------------------------------------"

remove_dir "target/debug/incremental" "Debug incremental cache"
remove_dir "target/release/incremental" "Release incremental cache"

echo ""

# ============================================
# 5. IDE and Editor Files
# ============================================

echo "💼 IDE/EDITOR FILES"
echo "----------------------------------------"

# Rust Analyzer cache
remove_dir ".rust-analyzer" "Rust Analyzer cache"

# VSCode cache
remove_dir ".vscode/.cache" "VSCode cache"

# IntelliJ/CLion
remove_dir ".idea" "IntelliJ IDEA cache"

# Vim/Neovim
remove_file ".*.swp" "Vim swap files"
rm -f .*.swp 2>/dev/null

echo ""

# ============================================
# 6. macOS Specific Files
# ============================================

if [[ "$(uname -s)" == "Darwin" ]]; then
    echo "🍎 MACOS SPECIFIC FILES"
    echo "----------------------------------------"
    
    # .DS_Store files
    echo "🗑️  Removing .DS_Store files"
    find . -name ".DS_Store" -type f -delete 2>/dev/null && echo "   ✅ Removed" || echo "   ⏭️  None found"
    
    echo ""
fi

# ============================================
# 7. Git Ignored Files (Optional - Super Aggressive)
# ============================================

if [[ "$1" == "--super-aggressive" ]] || [[ "$1" == "-sa" ]]; then
    echo "💥 SUPER AGGRESSIVE MODE - GIT CLEAN"
    echo "----------------------------------------"
    echo "⚠️  This will remove ALL untracked files!"
    echo ""
    
    if command -v git &> /dev/null && [[ -d ".git" ]]; then
        echo "Running: git clean -fdx"
        git clean -fdx
        echo "   ✅ Git clean completed"
    else
        echo "   ⏭️  Not a git repository"
    fi
    
    echo ""
fi

# ============================================
# Summary
# ============================================

echo "========================================="
echo "✅ CLEANUP COMPLETE"
echo "========================================="
echo ""

# Show what remains
if [[ -d "target" ]]; then
    echo "Remaining target/ size: $(get_size 'target')"
else
    echo "Target directory: Fully removed"
fi

echo ""
echo "💡 Cleanup modes:"
echo "   ./clean.sh                  - Standard cleanup"
echo "   ./clean.sh --aggressive     - Include Cargo cache"
echo "   ./clean.sh --super-aggressive - Remove ALL untracked files"
echo ""
echo "🔄 To rebuild: make build or ./build-engine.sh"
