#!/usr/bin/env bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║           RALPH MISSION CONTROL DASHBOARD                  ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Check if Python 3 is available
if command -v python3 &> /dev/null; then
    PYTHON=python3
elif command -v python &> /dev/null; then
    PYTHON=python
else
    echo "Error: Python is required to run the dashboard server."
    echo "Please install Python 3 and try again."
    exit 1
fi

echo "Starting server with $PYTHON..."
echo ""

cd "$SCRIPT_DIR"
$PYTHON server.py
