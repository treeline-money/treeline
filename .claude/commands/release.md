---
name: release
description: Release treeline (unified CalVer release of desktop, SDK, and CLI)
allowed-tools: Bash, Read, Glob, Grep, Edit, Write
argument-hint: [run_id]
---

# Release Treeline

Release Treeline with unified CalVer versioning. All components (desktop, SDK, CLI) are released together with the same version.

**Important**: This creates a **release candidate** that is NOT visible to users until you run `/promote`.

## CalVer Format

Treeline uses CalVer: `YY.M.DDRR` where DDRR = day × 100 + release (no leading zeros, npm-compatible)

| Component | Example | Description |
|-----------|---------|-------------|
| YY | 26 | Year (2026) |
| M | 1-12 | Month (no leading zero) |
| DDRR | 301, 3101 | Day × 100 + Release (1-99) |

Examples: `26.2.301` (Feb 3, release 1), `26.1.3101` (Jan 31, release 1), `26.12.1502` (Dec 15, release 2)

**Note**: Version is baked into artifacts at CI build time. Each CI run gets a unique CalVer based on date + run number.

## Usage

```bash
# Release the latest successful CI build
/release

# Release a specific CI run (by run ID)
/release 12345678
```

## Your Task

When the user invokes `/release`:

1. **Find the CI run to release**:
   ```bash
   # If run_id provided, use it; otherwise get latest successful
   if [ -n "$RUN_ID_ARG" ]; then
     RUN_ID="$RUN_ID_ARG"
   else
     RUN_ID=$(gh run list --workflow=ci.yml --branch=main --status=success --limit=1 --json databaseId --jq '.[0].databaseId')
   fi

   if [ -z "$RUN_ID" ]; then
     echo "No successful CI run found"
     exit 1
   fi

   # Get details about the run
   gh run view "$RUN_ID" --json headSha,createdAt,displayTitle --jq '.'
   ```

2. **Get the version from artifacts**:
   ```bash
   # Download version artifact
   gh run download "$RUN_ID" --name version --dir /tmp/release-version
   VERSION=$(cat /tmp/release-version/version.txt)
   echo "Version: $VERSION"
   rm -rf /tmp/release-version
   ```

3. **Check if already released**:
   ```bash
   if gh release view "$VERSION" --repo treeline-money/treeline &>/dev/null; then
     echo "Version $VERSION is already released"
     exit 1
   fi
   ```

4. **Generate release notes with two-pass refinement**:
   - Get commits since last release:
     ```bash
     PREV_TAG=$(gh release list --repo treeline-money/treeline --limit 1 --json tagName --jq '.[0].tagName')
     git log --pretty=format:"- %s" "${PREV_TAG}..HEAD" 2>/dev/null || git log --pretty=format:"- %s" -20
     ```
   - Generate **user-friendly** release notes (focus on what end-users will notice)
   - Keep notes simple and non-technical
   - **Show the generated notes to the user**
   - **Ask: "Any feedback on these notes? (Enter to accept, or describe changes)"**
   - Refine based on feedback if provided

5. **Confirm with user** (show version + final notes)

6. **Trigger the release workflow**:
   ```bash
   RUN_ID="<ci run id>"
   NOTES="<release notes>"

   gh workflow run release.yml \
     --repo treeline-money/treeline \
     -f run_id="$RUN_ID" \
     -f release_notes="$NOTES"
   ```

7. **Monitor and report**:
   - Watch workflow progress
   - If the release fails, check if the git tag was created (`git ls-remote --tags origin <version>`)
     and delete it (`git push origin --delete <version>`) before retrying
   - Report when complete with link to release

8. **Remind about next steps**:
   "Release workflow started. Once complete, test the auto-updater, then run `/promote` to make it available to users."

## Release Notes Format

```markdown
## What's New in <version>

### Improvements
- Faster startup time
- Better error messages when imports fail

### Bug Fixes
- Fixed an issue that prevented plugins from being installed
- Fixed sync errors with some bank connections
```

Keep notes:
- Concise (max 8 bullet points)
- Non-technical (no code terms, error messages)
- User-focused (what they'll notice)
- Skip version bumps, tests, internal refactoring

## What the Release Does

1. Downloads artifacts from specified CI run (version already baked in)
2. Creates git tag at the CI run's commit SHA
3. Creates GitHub releases in both `treeline` and `treeline-releases` (dual-publish)
4. Publishes SDK to npm (with provenance)
5. Publishes OpenClaw skill to ClawHub
6. Uploads `latest-staging.json` (RC - not visible to users until promoted)
7. Posts to Discord #staging-releases

## Release Flow

```
CI builds artifacts (version baked in) → /release tags a build → test auto-updater → /promote → users see update
```

## Example

```bash
/release
# → Finds latest successful CI run
# → Shows version (e.g., 26.2.301) from artifacts
# → Generates notes, asks for feedback
# → Triggers release workflow
# → Reports when complete
# → Reminds to test and run /promote
```
