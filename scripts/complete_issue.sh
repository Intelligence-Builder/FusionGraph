#!/bin/bash
# Post commit context to GitHub issue after implementation
# Usage: ./scripts/complete_issue.sh <issue-number> <file1> <file2> ...

set -e

ISSUE_NUM=$1
shift
FILES_CHANGED=$@

if [ -z "$ISSUE_NUM" ] || [ -z "$FILES_CHANGED" ]; then
    echo "Usage: $0 <issue-number> <file1> <file2> ..."
    echo "Example: $0 4 crates/fusiongraph-core/src/csr.rs"
    exit 1
fi

REPO="Intelligence-Builder/FusionGraph"
COMMIT_HASH=$(git log -1 --format="%H")
SHORT_HASH=$(git log -1 --format="%h")
COMMIT_MSG=$(git log -1 --format="%s")

FILE_LINKS=""
for file in $FILES_CHANGED; do
    FILE_LINKS="${FILE_LINKS}- [\`${file}\`](https://github.com/${REPO}/blob/${COMMIT_HASH}/${file})
"
done

echo "Adding commit reference to issue #${ISSUE_NUM}..."
gh issue comment $ISSUE_NUM --repo $REPO --body "### Commit Reference

Implementation: [${SHORT_HASH}](https://github.com/${REPO}/commit/${COMMIT_HASH})

**Files changed:**
${FILE_LINKS}
### Implementation Complete

${COMMIT_MSG}

**Verification:**
\`\`\`bash
cargo test
cargo clippy -- -D warnings
\`\`\`"

echo "Done. Issue #${ISSUE_NUM} updated."
