#!/usr/bin/env bash
#
# Setup script for hisiflash development environment
#
# This configures git hooks and other development settings.
#
# Usage:
#   ./scripts/setup-hooks.sh
#

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

GREEN='\033[0;32m'
NC='\033[0m'

info() { echo -e "${GREEN}[setup]${NC} $*"; }

cd "$PROJECT_ROOT"

# Configure git to use the .githooks directory
info "Setting git hooks path to .githooks/"
git config core.hooksPath .githooks

# Ensure hooks are executable
chmod +x .githooks/*

info "Git hooks installed successfully!"
info ""
info "Hooks configured:"
info "  pre-push — checks fmt, clippy, and tag-version consistency"
info ""
info "To skip hooks in emergency:"
info "  HISIFLASH_SKIP_HOOKS=1 git push ..."
