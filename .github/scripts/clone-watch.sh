#!/usr/bin/env bash
# Looks for unauthorized clones/impersonations of this repository elsewhere on GitHub.
# Run on a schedule (see .github/workflows/clone-watch.yml), independent of the build/CI flow.
#
# Two detection strategies:
#   1. Repos with the same name, not owned by us, not a fork.
#   2. Commits authored by the maintainer's email in repos owned by someone else
#      (catches renamed clones that still carry over the original commit history).
#
# Findings are filtered against .github/clone-watch-allowlist.txt (known legitimate
# collaborations) and deduplicated against already-open `clone-watch` issues before
# a new issue is filed.

set -euo pipefail

OWNER="${OWNER:?OWNER env var required}"
REPO_NAME="${REPO_NAME:?REPO_NAME env var required}"
AUTHOR_EMAIL="${AUTHOR_EMAIL:?AUTHOR_EMAIL env var required}"
ALLOWLIST_FILE="${ALLOWLIST_FILE:-.github/clone-watch-allowlist.txt}"
LABEL="clone-watch"

is_allowlisted() {
  local candidate_owner_lower
  candidate_owner_lower=$(echo "$1" | tr '[:upper:]' '[:lower:]')
  if [ -f "$ALLOWLIST_FILE" ]; then
    while IFS= read -r line; do
      line="${line%%#*}"
      line="$(echo "$line" | xargs || true)"
      [ -z "$line" ] && continue
      if [ "$(echo "$line" | tr '[:upper:]' '[:lower:]')" = "$candidate_owner_lower" ]; then
        return 0
      fi
    done < "$ALLOWLIST_FILE"
  fi
  return 1
}

echo "== Strategy 1: repos named '$REPO_NAME' not owned by $OWNER =="
name_matches=$(gh api "search/repositories?q=${REPO_NAME}+in:name&per_page=50" \
  --jq ".items[] | select(.owner.login != \"$OWNER\") | select(.fork == false) | \"\(.full_name)|\(.owner.login)|\(.html_url)|\(.created_at)|name-match\"")

echo "== Strategy 2: commits authored by $AUTHOR_EMAIL in repos not owned by $OWNER =="
commit_matches=$(gh api "search/commits?q=author-email:${AUTHOR_EMAIL}&per_page=50" \
  --jq ".items[] | select(.repository.owner.login != \"$OWNER\") | select(.repository.fork == false) | \"\(.repository.full_name)|\(.repository.owner.login)|\(.repository.html_url)|\(.repository.created_at // \"unknown\")|commit-history-match\"")

all_matches=$(printf '%s\n%s\n' "$name_matches" "$commit_matches" | sed '/^$/d' | sort -u -t'|' -k1,1)

if [ -z "$all_matches" ]; then
  echo "No matches found at all (allowlisted or not). Nothing to do."
  exit 0
fi

echo "== Existing open clone-watch issues (for dedup) =="
existing_issues_body=$(gh issue list --label "$LABEL" --state open --json body --jq '[.[].body] | join("\n---\n")' || true)

new_findings=""
while IFS= read -r match; do
  [ -z "$match" ] && continue
  full_name="${match%%|*}"
  owner="$(echo "$match" | cut -d'|' -f2)"

  if is_allowlisted "$owner"; then
    echo "skip (allowlisted): $full_name"
    continue
  fi

  if echo "$existing_issues_body" | grep -qF "$full_name"; then
    echo "skip (already reported in an open issue): $full_name"
    continue
  fi

  echo "NEW finding: $match"
  new_findings="${new_findings}${match}"$'\n'
done <<< "$all_matches"

new_findings=$(echo "$new_findings" | sed '/^$/d')

if [ -z "$new_findings" ]; then
  echo "All matches are allowlisted or already tracked. Nothing new to report."
  exit 0
fi

body="Automated scan found repositories that may be unauthorized clones of \`${OWNER}/${REPO_NAME}\`.

This does **not** automatically mean malicious intent - review each one manually before taking any action (e.g. reporting to GitHub via https://github.com/contact/report-abuse). See [SECURITY.md](../blob/main/SECURITY.md#malicious-forks--clones) for the criteria and process.

If a finding here is actually a legitimate collaboration, add its owner to \`.github/clone-watch-allowlist.txt\` to silence it.

| Repository | Owner | Detected via | Created | Link |
|---|---|---|---|---|
"

while IFS= read -r match; do
  [ -z "$match" ] && continue
  full_name="$(echo "$match" | cut -d'|' -f1)"
  owner="$(echo "$match" | cut -d'|' -f2)"
  url="$(echo "$match" | cut -d'|' -f3)"
  created="$(echo "$match" | cut -d'|' -f4)"
  reason="$(echo "$match" | cut -d'|' -f5)"
  body="${body}| \`${full_name}\` | \`${owner}\` | ${reason} | ${created} | ${url} |
"
done <<< "$new_findings"

echo "Creating issue with $(echo "$new_findings" | wc -l) new finding(s)..."
gh issue create \
  --title "[clone-watch] Possible unauthorized clone(s) detected - $(date -u +%Y-%m-%d)" \
  --label "$LABEL" \
  --body "$body"
