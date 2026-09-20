#!/usr/bin/env python3
# bin/stamp-cask-sha.py -- rewrites one asset's `sha256` line in Casks/txtodo-desktop.rb. Called by
# update-cask.sh once per .dmg, with ASSET_NAME (e.g. desktop-macos-aarch64.dmg), SHA and CASK
# (the cask's path) in the environment.
#
# Lives in its own file, not a heredoc in update-cask.sh, because `brew style`'s shell formatter
# (Library/Homebrew/utils/shfmt.sh, wrap_then_do) treats any heredoc line that starts with `if ` as
# the start of a shell `if` and swallows the rest of the script; `brew style --fix` then deletes it.
import os, re

path = os.environ["CASK"]
name = os.environ["ASSET_NAME"]
sha = os.environ["SHA"]

with open(path) as f:
    lines = f.readlines()

# The url line naming this exact asset (anchored on the trailing quote, same discipline
# update-formula.sh uses, so "aarch64" can never false-match "x86_64"'s own url/hash).
url_re = re.compile(re.escape(name) + r'"')
sha_re = re.compile(r'sha256 "[0-9a-f]+"')

for i, line in enumerate(lines):
    if url_re.search(line):
        # Walk back to the nearest preceding sha256 line — one asset's own, since each on_arm/
        # on_intel block holds exactly one sha256/url pair.
        for j in range(i - 1, -1, -1):
            if sha_re.search(lines[j]):
                lines[j] = sha_re.sub(f'sha256 "{sha}"', lines[j])
                break
        else:
            raise SystemExit(f"update-cask: no sha256 line found above the {name} url")
        break
else:
    raise SystemExit(f"update-cask: no url line found for {name}")

with open(path, "w") as f:
    f.writelines(lines)
