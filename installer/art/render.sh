#!/usr/bin/env bash
# Re-render installer art PNGs from art.html (run from Git Bash, needs Chrome). Output is committed;
# CI does not render. Scales cover 100%-300% display scaling.
set -e
cd "$(dirname "$0")"
D=$(cygpath -m "$PWD")
CHROME="/c/Program Files/Google/Chrome/Application/chrome.exe"
shot() { # id w h scale out
  "$CHROME" --headless=new --disable-gpu --hide-scrollbars --default-background-color=00000000 \
    --force-device-scale-factor="$4" --window-size="$2,$3" --screenshot="$D/$5" "file:///$D/art.html#$1" >/dev/null 2>&1
}
for s in 100 125 150 175 200 250 300; do
  f=$(awk "BEGIN{print $s/100}")
  shot side 164 314 "$f" "side-$s.png"
  shot small 55 55 "$f" "small-$s.png"
done
for s in 100 150 200 300; do
  f=$(awk "BEGIN{print $s/100}")
  shot support 440 58 "$f" "support-$s.png"
  shot topup 440 58 "$f" "topup-$s.png"
done
