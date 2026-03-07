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

3. Categorize each commit by its conventional-commit prefix:
   - `feat:` → **New Features**
   - `fix:` → **Bug Fixes**
   - `perf:` → **Performance**
   - `refactor:` → **Refactoring**
   - `docs:` → **Documentation**
   - `ci:` / `build:` → **CI & Build**
   - `chore:` → **Maintenance**
   - `BREAKING CHANGE` or `!:` → **Breaking Changes** (always listed first)
   - Uncategorized → **Other**

4. For each commit, note which crate(s) are affected by checking the changed files:
   ```bash
   git diff-tree --no-commit-id --name-only -r <hash> | grep '^crates/' | cut -d/ -f2 | sort -u
   ```

5. Output the release notes in this format:

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

6. If there are no commits since the last tag, say so.
