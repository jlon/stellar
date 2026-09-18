#!/usr/bin/env bash

#
# Stellar - Frontend Build Script
# Builds the Angular frontend and outputs to frontend/dist/
# Backend will directly embed from frontend/dist/ (no copy needed)
#

set -e

# Get project root
PROJECT_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
FRONTEND_DIR="$PROJECT_ROOT/frontend"

# Colors
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}Building Stellar Frontend${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""

# Skip switch for dev loops / fast backend-only verification:
#   SKIP_FRONTEND=1 bash build/build-frontend.sh
# The previous frontend/dist is reused as-is (embedded by the backend build).
if [ "${SKIP_FRONTEND:-0}" = "1" ]; then
    echo -e "${YELLOW}[skip]${NC} SKIP_FRONTEND=1: reusing existing frontend/dist
"
    exit 0
fi

echo -e "${YELLOW}[1/3]${NC} Installing frontend dependencies..."
cd "$FRONTEND_DIR"
if [ "${SKIP_NPM_INSTALL:-0}" = "1" ]; then
    echo -e "${YELLOW}[skip]${NC} SKIP_NPM_INSTALL=1: reusing existing node_modules"
elif [ "${CI:-}" = "true" ]; then
    npm ci
else
    npm install
fi

echo -e "${YELLOW}[2/3]${NC} Building Angular frontend (production mode)..."
# Use relative base href (./) to support sub-path deployments
# This follows Flink's approach: ng build --prod --base-href ./
# The relative base href allows the same build to work for both root (/) and sub-path (/xxx) deployments
echo "  Building with relative base href (./) for sub-path deployment support"
npm run build -- --configuration production --base-href ./

echo -e "${YELLOW}[3/3]${NC} Slimming embedded assets (release size)..."
cd "$FRONTEND_DIR/dist"
# Sourcemaps: production config disables them; drop any strays (keep the binary lean).
find . -name "*.map" -delete
# Fonts: keep only woff2 where available; keep legacy formats for families without woff2:
#   - *.eot: IE-only, drop all
#   - Roboto*/Exo*/fa-*: woff2 exists -> drop svg/ttf/woff
#   - ionicons/nebular/socicon: no woff2 -> keep woff+svg, drop ttf
#   - OpenSans: only ttf -> keep as-is
#   - root *.svg: legacy svg font faces (unsupported by all modern browsers) -> drop
find . -name "*.eot" -delete
find . -name "*.woff" ! -name "ionicons.woff" ! -name "nebular.woff" ! -name "socicon.woff" -delete
find . -name "*.ttf" ! -name "OpenSans*" ! -name "ionicons.ttf" ! -name "nebular.ttf" ! -name "socicon.ttf" -delete
find . -maxdepth 1 -name "*.svg" -delete

# Compress text assets in place (gzip -9): the backend detects the gzip magic
# bytes and serves them with Content-Encoding: gzip. Binary formats (woff2,
# png, jpg, ico...) are already compressed and are left untouched.
find . -type f \( -name "*.js" -o -name "*.css" -o -name "*.html" -o -name "*.json" -o -name "*.txt" -o -name "*.svg" \) -exec gzip -9f {} +

cd "$FRONTEND_DIR"
echo ""
echo -e "${GREEN}✓ Frontend build complete!${NC}"
echo -e "  Output: $FRONTEND_DIR/dist/ ($(du -sh "$FRONTEND_DIR/dist" | cut -f1))"
echo -e "  Note: Backend will embed directly from this directory"
