#!/bin/bash
# Benchmark script for kdownload
# Compares performance against wget, curl, and aria2

set -eo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

KDOWNLOAD="./target/release/kdownload"
BENCHMARK_DIR="./benchmark_results"
TEMP_DIR="/tmp/kdownload_bench_$$"

# Test files from public CDNs with good bandwidth
declare -A TEST_FILES=(
    ["10MB"]="https://proof.ovh.net/files/10Mb.dat"
    ["100MB"]="https://proof.ovh.net/files/100Mb.dat"
    ["1GB"]="https://proof.ovh.net/files/1Gb.dat"
)

# Create directories
mkdir -p "$BENCHMARK_DIR" "$TEMP_DIR"

echo -e "${BLUE}=== kdownload Benchmark Suite ===${NC}"
echo "Testing against: wget, curl, aria2c"
echo "Temp directory: $TEMP_DIR"
echo ""

# Check if tools are available
check_tool() {
    if command -v "$1" &> /dev/null; then
        echo -e "${GREEN}✓${NC} $1 is available"
        return 0
    else
        echo -e "${YELLOW}⚠${NC} $1 is not available (skipping)"
        return 1
    fi
}

echo "Checking available tools..."
check_tool "$KDOWNLOAD"
HAVE_WGET=0
if command -v wget &> /dev/null; then
    echo -e "${GREEN}✓${NC} wget is available"
    HAVE_WGET=1
else
    echo -e "${YELLOW}⚠${NC} wget is not available (skipping)"
fi

HAVE_CURL=0
if command -v curl &> /dev/null; then
    echo -e "${GREEN}✓${NC} curl is available"
    HAVE_CURL=1
else
    echo -e "${YELLOW}⚠${NC} curl is not available (skipping)"
fi

HAVE_ARIA2=0
if command -v aria2c &> /dev/null; then
    echo -e "${GREEN}✓${NC} aria2c is available"
    HAVE_ARIA2=1
else
    echo -e "${YELLOW}⚠${NC} aria2c is not available (skipping)"
fi
echo ""

# Benchmark function
benchmark() {
    local name="$1"
    local url="$2"
    local size_label="$3"
    local output_file="$TEMP_DIR/output_${size_label}"

    echo -e "${BLUE}Testing: $size_label file${NC}"

    # Clean up
    rm -f "$output_file"*

    # Measure time with GNU time if available, otherwise use built-in time
    local time_cmd="command time -f '%e %M' 2>&1"

    # kdownload
    echo -n "  kdownload:  "
    rm -f "$output_file"
    local start=$(date +%s.%N)
    $KDOWNLOAD -q "$url" -o "$output_file" 2>&1 > /dev/null || true
    local end=$(date +%s.%N)
    local kdownload_time=$(echo "$end - $start" | bc)
    local kdownload_speed=$(echo "scale=2; $size_label / $kdownload_time" | bc 2>/dev/null || echo "N/A")
    echo -e "${GREEN}${kdownload_time}s${NC} ($(printf "%.2f" $(echo "$kdownload_speed" | bc 2>/dev/null || echo 0)) MB/s)"

    # wget
    if [ "$HAVE_WGET" = "1" ]; then
        echo -n "  wget:       "
        rm -f "$output_file"
        local start=$(date +%s.%N)
        wget -q "$url" -O "$output_file" 2>&1 > /dev/null || true
        local end=$(date +%s.%N)
        local wget_time=$(echo "$end - $start" | bc)
        local wget_speed=$(echo "scale=2; $size_label / $wget_time" | bc 2>/dev/null || echo "N/A")
        echo -e "${GREEN}${wget_time}s${NC} ($(printf "%.2f" $(echo "$wget_speed" | bc 2>/dev/null || echo 0)) MB/s)"
    fi

    # curl
    if [ "$HAVE_CURL" = "1" ]; then
        echo -n "  curl:       "
        rm -f "$output_file"
        local start=$(date +%s.%N)
        curl -s "$url" -o "$output_file" 2>&1 > /dev/null || true
        local end=$(date +%s.%N)
        local curl_time=$(echo "$end - $start" | bc)
        local curl_speed=$(echo "scale=2; $size_label / $curl_time" | bc 2>/dev/null || echo "N/A")
        echo -e "${GREEN}${curl_time}s${NC} ($(printf "%.2f" $(echo "$curl_speed" | bc 2>/dev/null || echo 0)) MB/s)"
    fi

    # aria2c
    if [ "$HAVE_ARIA2" = "1" ]; then
        echo -n "  aria2c:     "
        rm -f "$output_file"
        local start=$(date +%s.%N)
        aria2c -q --dir="$TEMP_DIR" --out="output_${size_label}" "$url" 2>&1 > /dev/null || true
        local end=$(date +%s.%N)
        local aria2_time=$(echo "$end - $start" | bc)
        local aria2_speed=$(echo "scale=2; $size_label / $aria2_time" | bc 2>/dev/null || echo "N/A")
        echo -e "${GREEN}${aria2_time}s${NC} ($(printf "%.2f" $(echo "$aria2_speed" | bc 2>/dev/null || echo 0)) MB/s)"
    fi

    echo ""

    # Store results
    echo "$size_label,$kdownload_time,${wget_time:-N/A},${curl_time:-N/A},${aria2_time:-N/A}" >> "$BENCHMARK_DIR/results.csv"
}

# Initialize results file
echo "File Size,kdownload (s),wget (s),curl (s),aria2c (s)" > "$BENCHMARK_DIR/results.csv"

# Run benchmarks
for size in "10MB" "100MB"; do
    if [ -n "${TEST_FILES[$size]}" ]; then
        # Convert size to numeric (remove MB/GB suffix)
        size_numeric=$(echo "$size" | sed 's/MB$//' | sed 's/GB$//')
        [ "${size: -2}" = "GB" ] && size_numeric=$(echo "$size_numeric * 1024" | bc)

        benchmark "test_$size" "${TEST_FILES[$size]}" "$size_numeric"
        sleep 2  # Cool down between tests
    fi
done

# Clean up
rm -rf "$TEMP_DIR"

echo -e "${GREEN}Benchmark complete!${NC}"
echo "Results saved to: $BENCHMARK_DIR/results.csv"
echo ""
echo -e "${BLUE}Summary:${NC}"
cat "$BENCHMARK_DIR/results.csv" | column -t -s ','
