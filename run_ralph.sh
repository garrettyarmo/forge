#!/usr/bin/env bash
set -e

RALPH_DIR="/Users/garrettyarmowich/forge"
DASHBOARD_PORT=8888

cd "$RALPH_DIR" || exit 1

# Parse arguments
LAUNCH_DASHBOARD=false
for arg in "$@"; do
    case $arg in
        --dashboard|-d)
            LAUNCH_DASHBOARD=true
            shift
            ;;
    esac
done

echo ""
echo "╔════════════════════════════════════════════════════════════╗"
echo "║                    RALPH AGENT RUNNER                       ║"
echo "╚════════════════════════════════════════════════════════════╝"
echo ""

# Launch dashboard if requested
if [ "$LAUNCH_DASHBOARD" = true ]; then
    echo "🚀 Starting Mission Control dashboard..."
    python3 ralph-dashboard/server.py &
    DASHBOARD_PID=$!
    sleep 1
    echo "   Dashboard running at: http://localhost:$DASHBOARD_PORT"
    echo ""

    # Open browser (macOS)
    if command -v open &> /dev/null; then
        open "http://localhost:$DASHBOARD_PORT"
    fi

    # Trap to kill dashboard on exit
    trap "kill $DASHBOARD_PID 2>/dev/null || true" EXIT
fi

echo "🤖 Starting Ralph..."
echo "   Logs will be saved to: ralph-logs/current-run.jsonl"
echo ""

# Run ralph with caffeinate to prevent sleep
caffeinate -i ./ralph.sh

echo ""
echo "✅ Ralph run complete!"
echo "   View archived logs: ls ralph-logs/"
