#!/bin/bash
set -e

VERSION=$1

if [ -z "$VERSION" ]; then
  echo "Usage: ./scripts/release.sh <version>"
  echo "Example: ./scripts/release.sh 0.1.0"
  exit 1
fi

# Ensure version starts with 'v'
if [[ ! "$VERSION" =~ ^v ]]; then
  VERSION="v$VERSION"
fi

# Update version in manifest.json
MANIFEST_VERSION="${VERSION#v}"
if command -v jq &> /dev/null; then
  jq ".version = \"$MANIFEST_VERSION\"" manifest.json > manifest.tmp && mv manifest.tmp manifest.json
  echo "Updated manifest.json version to $MANIFEST_VERSION"
else
  echo "Warning: jq not installed, please manually update version in manifest.json"
fi

# Build to verify everything works
echo "Building plugin..."
npm run build

# Commit version bump if there are changes
if ! git diff --quiet manifest.json 2>/dev/null; then
  git add manifest.json
  git commit -m "Bump version to $VERSION"
fi

# Create and push tag
echo "Creating tag $VERSION..."
git tag "$VERSION"
git push origin main --tags

echo ""
echo "Release $VERSION created!"
echo "GitHub Actions will build and publish the release automatically."
echo "Check: https://github.com/$(git remote get-url origin | sed 's/.*github.com[:/]\(.*\)\.git/\1/')/actions"
