#!/usr/bin/env bash
# Run by hand before working on the engine pin. Not on a timer.
#
# Answers three questions before anything else is trusted:
#   1. Is the engine we pin still upstream's latest release?
#   2. Has anything moved on the issues this project is waiting on?
#   3. Has upstream main moved past the tag (an unreleased change coming)?
set -euo pipefail

REPO=btxchain/btx
PINNED_TAG=v0.34.16
PINNED_COMMIT=074dac16
WATCH=(225 226 227)

latest=$(gh release view --repo "$REPO" --json tagName --jq .tagName)
main=$(gh api "repos/$REPO/commits/main" --jq '.sha[0:8]')

echo "pinned:  $PINNED_TAG ($PINNED_COMMIT)"
echo "latest:  $latest"
echo "main:    $main"
[[ "$latest" == "$PINNED_TAG" ]] || echo "!! upstream released $latest. Read its notes before doing anything else."
[[ "$main" == "$PINNED_COMMIT" ]] || echo "!! main moved past the pinned commit. Check what landed: gh api repos/$REPO/compare/$PINNED_COMMIT...main"

echo
for n in "${WATCH[@]}"; do
  gh api "repos/$REPO/issues/$n" \
    --jq '"#\(.number) \(.state)  comments=\(.comments)  updated=\(.updated_at[0:16])  \(.title)"'
done
