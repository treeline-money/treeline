#!/bin/bash
set -e

# Treeline Release Script
# Automates version bumping, tagging, and GitHub release creation
# Usage: ./scripts/release.sh <version>
# Example: ./scripts/release.sh v0.2.0

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Check if version argument provided
if [ -z "$1" ]; then
    echo -e "${RED}Error: Version argument required${NC}"
    echo "Usage: ./scripts/release.sh <version>"
    echo "Example: ./scripts/release.sh v0.2.0"
    exit 1
fi

VERSION=$1

# Remove 'v' prefix if present for pyproject.toml
VERSION_NUMBER=${VERSION#v}

# Validate version format (should be semver)
if ! [[ $VERSION_NUMBER =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
    echo -e "${RED}Error: Invalid version format. Use semver (e.g., 0.2.0 or v0.2.0)${NC}"
    exit 1
fi

# Ensure we're on main branch
CURRENT_BRANCH=$(git branch --show-current)
if [ "$CURRENT_BRANCH" != "main" ]; then
    echo -e "${RED}Error: Must be on main branch to create a release${NC}"
    echo "Current branch: $CURRENT_BRANCH"
    exit 1
fi

# Ensure working directory is clean
if ! git diff-index --quiet HEAD --; then
    echo -e "${RED}Error: Working directory has uncommitted changes${NC}"
    echo "Please commit or stash changes before releasing"
    git status --short
    exit 1
fi

# Pull latest changes
echo -e "${YELLOW}Pulling latest changes from origin/main...${NC}"
git pull origin main

# Check if gh CLI is installed
if ! command -v gh &> /dev/null; then
    echo -e "${RED}Error: GitHub CLI (gh) is not installed${NC}"
    echo "Install with: brew install gh"
    exit 1
fi

# Check if gh is authenticated
if ! gh auth status &> /dev/null; then
    echo -e "${RED}Error: GitHub CLI is not authenticated${NC}"
    echo "Run: gh auth login"
    exit 1
fi

echo -e "${GREEN}Creating release ${VERSION}${NC}"
echo ""

# Update version in cli/pyproject.toml
echo -e "${YELLOW}Updating version in cli/pyproject.toml...${NC}"
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    sed -i '' "s/^version = \".*\"/version = \"${VERSION_NUMBER}\"/" cli/pyproject.toml
else
    # Linux
    sed -i "s/^version = \".*\"/version = \"${VERSION_NUMBER}\"/" cli/pyproject.toml
fi

# Verify the CLI change
UPDATED_VERSION=$(grep "^version = " cli/pyproject.toml | cut -d'"' -f2)
if [ "$UPDATED_VERSION" != "$VERSION_NUMBER" ]; then
    echo -e "${RED}Error: Failed to update version in pyproject.toml${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Updated CLI version to ${VERSION_NUMBER}${NC}"

# Update version in plugin-sdk/package.json
echo -e "${YELLOW}Updating version in plugin-sdk/package.json...${NC}"
if [[ "$OSTYPE" == "darwin"* ]]; then
    # macOS
    sed -i '' "s/\"version\": \".*\"/\"version\": \"${VERSION_NUMBER}\"/" plugin-sdk/package.json
else
    # Linux
    sed -i "s/\"version\": \".*\"/\"version\": \"${VERSION_NUMBER}\"/" plugin-sdk/package.json
fi

# Verify the SDK change
SDK_UPDATED_VERSION=$(grep '"version":' plugin-sdk/package.json | head -1 | cut -d'"' -f4)
if [ "$SDK_UPDATED_VERSION" != "$VERSION_NUMBER" ]; then
    echo -e "${RED}Error: Failed to update version in plugin-sdk/package.json${NC}"
    exit 1
fi
echo -e "${GREEN}✓ Updated plugin-sdk version to ${VERSION_NUMBER}${NC}"

# Update uv.lock to match new version
echo -e "${YELLOW}Updating uv.lock...${NC}"
cd cli
uv lock
cd ..
echo -e "${GREEN}✓ Updated uv.lock${NC}"

# Commit version bump
echo -e "${YELLOW}Committing version bump...${NC}"
git add cli/pyproject.toml cli/uv.lock plugin-sdk/package.json
git commit -m "Bump version to ${VERSION}"
echo -e "${GREEN}✓ Committed version bump${NC}"

# Create git tag
echo -e "${YELLOW}Creating git tag ${VERSION}...${NC}"
git tag -a "${VERSION}" -m "Release ${VERSION}"
echo -e "${GREEN}✓ Created tag ${VERSION}${NC}"

# Push commits and tags
echo -e "${YELLOW}Pushing to origin...${NC}"
git push origin main
git push origin "${VERSION}"
echo -e "${GREEN}✓ Pushed commits and tags${NC}"

# Generate release notes
echo -e "${YELLOW}Generating release notes...${NC}"

# Get the previous tag
PREV_TAG=$(git describe --tags --abbrev=0 "${VERSION}^" 2>/dev/null || echo "")

if [ -n "$PREV_TAG" ]; then
    COMMITS=$(git log --pretty=format:"- %s" "${PREV_TAG}..${VERSION}" 2>/dev/null | head -50)
else
    COMMITS=$(git log --pretty=format:"- %s" -20 2>/dev/null)
fi

# Create release notes file
NOTES_FILE=$(mktemp)
trap "rm -f $NOTES_FILE" EXIT

# Check if claude CLI is available for AI-generated notes
CLAUDE_CMD=""
if [ -x "$HOME/.claude/local/claude" ]; then
    CLAUDE_CMD="$HOME/.claude/local/claude"
elif command -v claude &> /dev/null; then
    CLAUDE_CMD="claude"
fi

if [ -n "$CLAUDE_CMD" ]; then
    echo -e "${YELLOW}Using Claude to generate user-friendly release notes...${NC}"

    PROMPT="Generate concise, user-friendly release notes for Treeline CLI (a personal finance CLI tool) version ${VERSION}.

Here are the commits since the last release:
${COMMITS}

Write release notes in this format:
## What's New in ${VERSION}

### [Category like 'Bug Fixes', 'New Features', 'Improvements']
- Brief, user-friendly description of change (no technical jargon)

Keep it concise (max 10 bullet points). Focus on what users will notice, not implementation details.
Skip version bump commits. If there are no user-facing changes, just write 'Minor improvements and bug fixes.'"

    # Use claude CLI to generate notes
    # Note: Using background job + wait with timeout for macOS compatibility
    if ( "$CLAUDE_CMD" -p "$PROMPT" > "$NOTES_FILE" 2>/dev/null ) &
       CLAUDE_PID=$!
       ( sleep 60 && kill $CLAUDE_PID 2>/dev/null ) &
       TIMEOUT_PID=$!
       wait $CLAUDE_PID 2>/dev/null
       kill $TIMEOUT_PID 2>/dev/null
       [ -s "$NOTES_FILE" ]; then
        echo -e "${GREEN}✓ Generated AI release notes${NC}"
    else
        echo -e "${YELLOW}Claude generation failed or timed out, using commit list${NC}"
        echo "## What's New in ${VERSION}" > "$NOTES_FILE"
        echo "" >> "$NOTES_FILE"
        echo "${COMMITS}" >> "$NOTES_FILE"
    fi
else
    echo -e "${YELLOW}Claude CLI not found, using commit list${NC}"
    echo "## What's New in ${VERSION}" > "$NOTES_FILE"
    echo "" >> "$NOTES_FILE"
    echo "${COMMITS}" >> "$NOTES_FILE"
fi

echo ""
echo -e "${YELLOW}Generated release notes:${NC}"
echo "----------------------------------------"
cat "$NOTES_FILE"
echo "----------------------------------------"
echo ""

# Ask user to confirm or edit
read -p "Proceed with these release notes? (y/n/e to edit): " CONFIRM
case $CONFIRM in
    [eE])
        # Open in default editor
        ${EDITOR:-nano} "$NOTES_FILE"
        ;;
    [yY])
        # Continue
        ;;
    *)
        echo -e "${RED}Release cancelled${NC}"
        exit 1
        ;;
esac

# Create GitHub release with generated notes
echo -e "${YELLOW}Creating GitHub release...${NC}"
gh release create "${VERSION}" \
    --title "Release ${VERSION}" \
    --notes-file "$NOTES_FILE" \
    --verify-tag

echo ""
echo -e "${GREEN}✓ Release ${VERSION} created successfully!${NC}"
echo ""
echo "The GitHub Actions workflow will now:"
echo "  1. Run tests"
echo "  2. Publish CLI to PyPI"
echo "  3. Publish plugin-sdk to npm"
echo "  4. Build CLI binaries for all platforms"
echo "  5. Upload all binaries to the GitHub Release"
echo ""
echo "Monitor progress at: https://github.com/treeline-money/treeline/actions"
echo "View release at: https://github.com/treeline-money/treeline/releases/tag/${VERSION}"
