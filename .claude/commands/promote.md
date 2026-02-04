---
name: promote
description: Promote a release candidate to production (copies latest-staging.json to latest.json)
allowed-tools: Bash, Read, Edit
---

# Promote Release Candidate to Production

After testing a release candidate, use this skill to make it available to all users.

## What This Does

1. Triggers the promote workflow which:
   - Uploads `latest-staging.json` as `latest.json` to both repos
   - Posts to Discord #releases
2. Updates the download page at `treeline.money`
3. Users' auto-updaters will now see the new version

## Prerequisites

- A release must have been created via `/release`
- You should have tested the release candidate before promoting

## Testing the RC

Before promoting, test the auto-updater:

```bash
# Enable staging updates
touch ~/.treeline/use-staging-updates

# Open the app, check for updates
# The app should see the RC version

# Disable when done testing
rm ~/.treeline/use-staging-updates
```

## Your Task

When the user invokes `/promote`:

1. **Check for staging release**:
   ```bash
   # Get the latest release version from treeline repo
   VERSION=$(gh release list --repo treeline-money/treeline --limit 1 --json tagName --jq '.[0].tagName')
   echo "Latest release: $VERSION"

   # Check if latest-staging.json exists
   gh release view "$VERSION" --repo treeline-money/treeline --json assets --jq '.assets[].name' | grep latest-staging.json
   ```

2. **Check current latest.json version** (if it exists):
   ```bash
   gh release download "$VERSION" --repo treeline-money/treeline --pattern "latest.json" --dir /tmp --clobber 2>/dev/null || echo "No latest.json yet"
   cat /tmp/latest.json 2>/dev/null | jq -r '.version' || echo "N/A"
   ```
   If latest.json already shows the current version, it was already promoted.

3. **Confirm with user**:
   - Show the version being promoted
   - Ask for confirmation before proceeding

4. **Trigger the promote workflow**:
   ```bash
   gh workflow run promote.yml \
     --repo treeline-money/treeline \
     -f version="$VERSION"
   ```

5. **Monitor workflow**:
   - Wait for workflow to complete
   - Report success or failure

6. **Update the download page version**:
   ```bash
   DOWNLOAD_PAGE="$CLAUDE_PROJECT_DIR/treeline.money/src/pages/download.astro"

   # Update the VERSION constant
   sed -i '' "s/const VERSION = \"[^\"]*\"/const VERSION = \"$VERSION\"/" "$DOWNLOAD_PAGE"

   # Commit and push
   cd "$CLAUDE_PROJECT_DIR/treeline.money"
   git add src/pages/download.astro
   git commit -m "Update download page to $VERSION"
   git push
   ```

7. **Report success**:
   - Confirm the release is now live
   - Confirm download page was updated
   - Remind user that auto-updaters will pick up the new version

## Example Output

```
Checking for release candidate...
Found: 26.2.301 with latest-staging.json

Ready to promote 26.2.301 to production?
This will:
- Make the update available to all users via auto-updater
- Post to Discord #releases
- Update treeline.money/download

[User confirms]

Triggering promote workflow...
✓ Workflow started: https://github.com/treeline-money/treeline/actions/runs/...

Waiting for workflow...
✓ Workflow completed successfully

Updating download page...
✓ Updated download page to 26.2.301
✓ Pushed to treeline.money (Vercel will deploy)

✓ Release 26.2.301 is now live!

Users will receive the update via auto-updater.
```

## Rollback

If something goes wrong after promoting:

1. Delete `latest.json` from the release:
   ```bash
   gh release delete-asset <version> latest.json --repo treeline-money/treeline
   gh release delete-asset <version> latest.json --repo treeline-money/treeline-releases
   ```

2. Users will stay on their current version until next release

3. Fix the issue and create a new release
