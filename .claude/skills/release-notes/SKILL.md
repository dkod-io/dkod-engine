---
name: release-notes
description: Generate categorized release notes from git history since the last tag
disable-model-invocation: true
---

Generate release notes for the next version of dkod-engine.

## Steps

1. Find the latest tag:
   ```bash
   git tag --list 'v*' --sort=-version:refname | head -1
   ```
   If no tags exist, skip to step 2 using `--all` (no range filter) to include every commit.

2. Get all commits since that tag:
   ```bash
   git log <last-tag>..HEAD --pretty=format:"%h %s" --no-merges
   ```
   If no tags were found in step 1, list all commits (no range needed):
   ```bash
   git log --pretty=format:"%h %s" --no-merges
   ```

3. Determine the next version from the last tag and commit types:
   - If any commit contains `BREAKING CHANGE` or uses `!:` → bump **major** version
   - If any commit starts with `feat:` → bump **minor** version
   - Otherwise → bump **patch** version
   - If no previous tag exists, default to `v0.1.0`
   - Ask the user to confirm or override before generating notes.

4. Categorize each commit by its conventional-commit prefix:
   - `feat:` → **New Features**
   - `fix:` → **Bug Fixes**
   - `perf:` → **Performance**
   - `refactor:` → **Refactoring**
   - `docs:` → **Documentation**
   - `ci:` / `build:` → **CI & Build**
   - `chore:` → **Maintenance**
   - `BREAKING CHANGE` or `!:` → **Breaking Changes** (always listed first)
   - Uncategorized → **Other**

5. For each commit, note which crate(s) are affected by checking the changed files:
   ```bash
   git diff-tree --no-commit-id --name-only -r <hash> | grep '^crates/' | cut -d/ -f2 | sort -u
   ```

6. Output the release notes in this format:

```markdown
## v<next-version>

### Breaking Changes
- description (dk-core, dk-server) — #hash

### New Features
- description (dk-engine) — #hash

### Bug Fixes
- description (dk-cli) — #hash

...

**Full Changelog**: `<last-tag>..v<next-version>`
```

7. If there are no commits since the last tag, say so.
