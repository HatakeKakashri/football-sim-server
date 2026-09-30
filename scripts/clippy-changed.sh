#!/usr/bin/env bash

TARGET_REF="${1:-origin/main}"

echo "🔍 Fetching git diff against: $TARGET_REF"

# Get changed files safely
CHANGED_FILES=$(git diff --name-only "$TARGET_REF")
if [ $? -ne 0 ]; then
    echo "❌ Git diff failed. Please check if '$TARGET_REF' exists."
    exit 1
fi

if [ -z "$CHANGED_FILES" ]; then
    echo "✅ No files changed against $TARGET_REF."
    exit 0
fi

# Track packages to check
declare -A PACKAGES

# Loop through changed files directly
for file in $CHANGED_FILES; do
    # Skip files that don't exist anymore (deleted files)
    if [ ! -f "$file" ]; then continue; fi

    # Find the nearest Cargo.toml going upward
    dir=$(dirname "$file")
    while [ "$dir" != "." ] && [ "$dir" != "/" ] && [ -n "$dir" ]; do
        if [ -f "$dir/Cargo.toml" ]; then
            # Extract package name cleanly
            pkg_name=$(grep -m 1 '^name' "$dir/Cargo.toml" | tr -d '"' | awk '{print $3}')
            if [ -n "$pkg_name" ]; then
                PACKAGES["$pkg_name"]=1
            fi
            break
        fi
        dir=$(dirname "$dir")
    done
done

if [ ${#PACKAGES[@]} -eq 0 ]; then
    echo "ℹ️  No Rust packages were modified in this diff."
    exit 0
fi

echo "📦 Packages to lint: ${!PACKAGES[*]}"

# Run clippy on detected packages
for pkg in "${!PACKAGES[@]}"; do
    echo "----------------------------------------"
    echo "Linting: $pkg"
    echo "----------------------------------------"
    cargo clippy -p "$pkg" --no-deps -- -D warnings
done

