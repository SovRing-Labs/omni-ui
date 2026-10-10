#!/bin/sh
# SPDX-License-Identifier: MIT OR Apache-2.0
# pin-engine.sh [rev|live] — what OmniUI compiles the OMNI-RENDER engine from.
#   rev (default HEAD): `git archive` of projects/omni-render at that commit into .engine/ (gitignored build input; the engine
#                       repo is only read). Committed engine code only — the engine's owner's uncommitted work cannot break us.
#   live:               .engine/src -> ../../omni-render/src (track the working tree; may break while that agent is mid-edit)
set -e
cd "$(dirname "$0")/.."
ENG=../omni-render
REV="${1:-HEAD}"
rm -rf .engine && mkdir -p .engine
if [ "$REV" = live ]; then
  ln -s "../$ENG/src" .engine/src
  echo live > .engine/REV
else
  SHA=$(git -C "$ENG" rev-parse "$REV")
  git -C "$ENG" archive "$SHA" src | tar -x -C .engine
  echo "$SHA" > .engine/REV
fi
echo "engine pinned: $(cat .engine/REV)"
