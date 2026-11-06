#!/bin/bash
# USB export benchmark: Bake'n Deck's engine (baken expressport 4.8.0, the code inside Bake'n Deck 1.4.0) against rbxport,
# the same rekordbox playlist onto the same USB stick, plus an audit of every file each tool writes outside the stick.
#
# Run it as a dedicated macOS test user (never on your working library: rbxport opens the rekordbox library it finds),
# in Terminal, with rekordbox closed and every drive that holds another rekordbox library ejected:
#   bench.sh check                         preconditions only
#   bench.sh first-launch                  audit rbxport's first launch (you open and quit it by hand when asked)
#   bench.sh run [full|resync|convert|all] timed runs, REPS of each (default 3), tools alternating which goes first
#   bench.sh report                        results/summary.md
#
# Scenarios. full: an emptied stick, the whole playlist, export.pdb plus OneLibrary (rbxport always writes both, so
# baken gets --onelibrary). resync: the same export again onto the stick the same tool just wrote. convert: an emptied
# stick with conversion for older players, baken --cdjsafe against rbxport's Maximum CDJ compatibility set to MP3
# (both 320 kbps CBR), rbxport's conversion cache emptied before each run so every run is cold.
set -euo pipefail

B=/Users/Shared/rbbench
BIN="$B/bin"
XML="$B/work/collection.xml"
PLAYLIST="${PLAYLIST:-bench}"
STICK="${STICK:-BAKEN}"
VOL="${VOL:-/Volumes/$STICK}"
REPS="${REPS:-3}"
COOL="${COOL:-45}"
OUT="${OUT:-$B/results}"
CSV="$OUT/runs.csv"
BAKEN="${BAKEN:-$BIN/baken}"
export PATH="/opt/homebrew/bin:$PATH"   # baken --cdjsafe runs ffmpeg from PATH; the app bundles its own build, which only runs inside the app
PY=/usr/bin/python3   # the system Python (3.9) every user has; no newer syntax anywhere below
LIB=("$PY" -I "${BENCHLIB:-$BIN/benchlib.py}")
ROOTS=("$HOME/Library/Pioneer" "$HOME/Library/Application Support/Pioneer" "$B/music")
RBX_CACHE="$HOME/Library/Application Support/rbxport/backups/cache/usb-export"

die() { echo "error: $*" >&2; exit 1; }
now() { "$PY" -c 'import time; print("%.3f" % time.time())'; }
sub() { "$PY" -c 'import sys; print("%.2f" % (float(sys.argv[1]) - float(sys.argv[2])))' "$1" "$2"; }
jget() {   # json key: a dict prints its sorted keys, a number or text prints as is
  "$PY" -c 'import json,sys; v=json.loads(sys.argv[1])[sys.argv[2]]; print(" ".join(sorted(v)) if isinstance(v, dict) else v)' "$1" "$2"
}
rbx_app() {   # never fails: under pipefail a missing ~/Applications must not end the script
  local a
  for a in /Applications/rbxport.app "$HOME/Applications/rbxport.app"; do
    [ -d "$a" ] && { echo "$a"; return 0; }
  done
  return 0
}

check() {
  # rbxport looks for rekordbox libraries on every connected drive, so only the test user's own library may be reachable.
  for v in /Volumes/*; do
    [ "$v" = "$VOL" ] && continue
    [ ! -e "$v/PIONEER/Master/master.db" ] || die "$v holds a rekordbox library; eject it"
  done
  ! pgrep -xq rekordbox || die "quit rekordbox first"
  local settings="$HOME/Library/Application Support/Pioneer/rekordbox6/rekordbox3.settings"
  if [ -f "$settings" ] && grep -q 'name="masterDbDirectory"' "$settings"; then
    grep -q "name=\"masterDbDirectory\" val=\"$HOME/Library/Pioneer/rekordbox\"" "$settings" \
      || die "rekordbox's library is not the test user's own (masterDbDirectory in $settings)"
  fi
  [ -f "$HOME/Library/Pioneer/rekordbox/master.db" ] || die "no rekordbox library for this user yet: run rekordbox setup first"
  [ -f "$XML" ] || die "missing $XML: export the collection from rekordbox (File > Export Collection in xml format)"
  grep -q "Name=\"$PLAYLIST\"" "$XML" || die "playlist \"$PLAYLIST\" not in $XML"
  for f in MYSETTING.DAT MYSETTING2.DAT DJMMYSETTING.DAT; do
    [ -f "$HOME/Library/Application Support/Pioneer/rekordbox6/$f" ] \
      || die "My Settings missing ($f): in rekordbox, EXPORT mode > Preferences > DJ System > My Settings, change one value and change it back"
  done
  [ -d "$VOL" ] || die "stick $VOL not mounted"
  diskutil info "$VOL" | grep -q 'MS-DOS FAT32' || die "$VOL is not FAT32"
  [ -x "$BAKEN" ] || die "missing $BAKEN"
  command -v ffmpeg >/dev/null && command -v ffprobe >/dev/null || die "ffmpeg and ffprobe are needed on PATH for the convert runs"
  [ -n "$(rbx_app)" ] || die "rbxport.app not found in /Applications or ~/Applications"
  mkdir -p "$OUT"
  echo "ok: $(id -un), stick $VOL, playlist $PLAYLIST, $(grep -c '<TRACK ' "$XML") TRACK entries in the XML"
}

environment() {
  local app; app="$(rbx_app)"
  {
    echo "date: $(date -u '+%Y-%m-%d %H:%M UTC')"
    echo "machine: $(sysctl -n hw.model), $(sysctl -n machdep.cpu.brand_string), $(($(sysctl -n hw.memsize) / 1073741824)) GB"
    echo "macOS: $(sw_vers -productVersion) ($(sw_vers -buildVersion))"
    echo "power: $(pmset -g batt | head -1 | sed "s/.*'\(.*\)'.*/\1/")"
    echo "baken: $("$BAKEN" --version)"
    echo "ffmpeg (for baken --cdjsafe): $(ffmpeg -hide_banner -version | head -1)"
    echo "rbxport: $(defaults read "$app/Contents/Info.plist" CFBundleShortVersionString) ($app)"
    echo "rekordbox: $(defaults read "/Applications/rekordbox 7/rekordbox.app/Contents/Info.plist" CFBundleShortVersionString 2>/dev/null || echo '?')"
    local whole; whole=$(diskutil info "$VOL" | awk '/Part of Whole/{print $4}')
    echo "stick: $(diskutil info "$whole" | awk -F: '/Device \/ Media Name/{gsub(/^ +/,"",$2); print $2}'), $(diskutil info "$VOL" | awk -F: '/Disk Size/{gsub(/^ +/,"",$2); print $2}' | cut -d'(' -f1| sed 's/ *$//'), FAT32, $(diskutil info "$whole" | awk -F: '/Protocol/{gsub(/^ +/,"",$2); print $2}')"
    echo "corpus: $(ls "$B/music" | wc -l | tr -d ' ') files, $(du -sh "$B/music" | cut -f1)"
  } > "$OUT/environment.txt"
  cat "$OUT/environment.txt"
}

clear_stick() {   # everything an export can write; macOS's own hidden folders (.TemporaryItems, .Trashes, ...) stay
  find "$VOL" -mindepth 1 -maxdepth 1 ! -name '.*' -exec rm -rf {} + || true
  find "$VOL" -mindepth 1 -maxdepth 1 -name '._*' ! -name ._.metadata_never_index -exec rm -f {} + || true
  sync
  [ ! -e "$VOL/PIONEER" ] && [ ! -e "$VOL/Contents" ] || die "could not empty $VOL"
}

audit_start() { "${LIB[@]}" snapshot "$OUT/snap-$1-pre.tsv" "${ROOTS[@]}"; touch "$OUT/.marker-$1"; }
audit_end() {
  "${LIB[@]}" snapshot "$OUT/snap-$1-post.tsv" "${ROOTS[@]}"
  "${LIB[@]}" diff "$OUT/snap-$1-pre.tsv" "$OUT/snap-$1-post.tsv" > "$OUT/diff-$1.txt" || true
  find "$HOME" -newer "$OUT/.marker-$1" -type f ! -path "$HOME/Library/Caches/*" 2>/dev/null > "$OUT/newer-$1.txt" || true
  grep -vc '^mtime only' "$OUT/diff-$1.txt" || true
}

row() {   # tool scenario rep status wall wall_sync cpu rss_one_kb rss_sum_kb outside stickjson
  [ -f "$CSV" ] || echo "tool,scenario,rep,status,wall,wall_sync,cpu,rss_one_kb,rss_sum_kb,outside_writes,audio,anlz,stick_bytes,dbs" > "$CSV"
  local s="${11}"
  local audio anlz bytes dbs
  audio=$(jget "$s" audio); anlz=$(jget "$s" anlz); bytes=$(jget "$s" bytes); dbs=$(jget "$s" dbs)
  echo "$1,$2,$3,$4,$5,$6,$7,$8,$9,${10},$audio,$anlz,$bytes,$dbs" >> "$CSV"
}

run_baken() {   # scenario rep
  local scn=$1 rep=$2 id="bakendeck-$1-$2"
  local args=("$XML" --device "$VOL" --playlist "$PLAYLIST" --onelibrary)
  [ "$scn" = convert ] && args+=(--cdjsafe)
  [ "$scn" = resync ] || clear_stick
  audit_start "$id"
  local t0 t1 t2 st=0
  t0=$(now)
  /usr/bin/time -l "$BAKEN" expressport "${args[@]}" > "$OUT/log-$id.txt" 2> "$OUT/time-$id.txt" || st=$?
  t1=$(now); sync; t2=$(now)
  local ru outside stats
  ru=$("${LIB[@]}" rusage "$OUT/time-$id.txt")
  outside=$(audit_end "$id")
  stats=$("${LIB[@]}" stick "$VOL")
  local cpu rss
  cpu=$(sub "$(jget "$ru" user)" "-$(jget "$ru" sys)")
  rss=$(jget "$ru" peak_rss_one_kb)
  row bakendeck "$scn" "$rep" "$st" "$(sub "$t1" "$t0")" "$(sub "$t2" "$t0")" "$cpu" "$rss" "$rss" "$outside" "$stats"
  echo "  bakendeck $scn #$rep: exit $st, $(sub "$t1" "$t0") s, $outside file(s) written outside the stick"
}

rbx() { osascript -e "tell application id \"com.rbxport.app\"" -e "$1" -e "end tell"; }

rbx_setting() {   # name, AppleScript literal (false, true, "mp3"): waits until the window has handed its preferences
  local name=$1 lit=$2 want cur="" i            # to the scripting bridge (writes are refused before that), writes only
  want=$(printf '%s' "$lit" | tr -d '"')        # when the value differs, and falls back to the Preferences window
  for i in $(seq 90); do
    cur=$(rbx "get value of setting \"$name\"" 2>/dev/null || true)
    [ -n "$cur" ] && [ "$cur" != "missing value" ] && break
    sleep 1
  done
  [ "$cur" = "$want" ] && return 0
  for i in 1 2 3 4 5; do
    rbx "set value of setting \"$name\" to $lit" >/dev/null 2>&1 || true
    cur=$(rbx "get value of setting \"$name\"" 2>/dev/null || true)
    [ "$cur" = "$want" ] && return 0
    sleep 3
  done
  echo "  rbxport refused to set $name to $want from a script (it reads $cur)."
  echo "  Set it by hand in rbxport > Preferences > USB Export, then press Return here."
  read -r
  cur=$(rbx "get value of setting \"$name\"" 2>/dev/null || true)
  [ "$cur" = "$want" ] || die "$name is still $cur"
}

rbx_start() {
  if ! pgrep -xq rbxport; then open -a "$(rbx_app)"; fi
  local i
  for i in $(seq 120); do rbx 'count playlists' >/dev/null 2>&1 && return 0; sleep 1; done
  die "rbxport did not answer AppleScript within two minutes (check System Settings > Privacy & Security > Automation)"
}

rbx_quit() {
  pgrep -xq rbxport || return 0
  rbx 'quit' >/dev/null 2>&1 || true
  local i
  for i in $(seq 30); do pgrep -xq rbxport || return 0; sleep 1; done
  pkill -x rbxport || true
  for i in $(seq 30); do pgrep -xq rbxport || return 0; sleep 1; done
  die "rbxport did not quit"
}

run_rbxport() {   # scenario rep
  local scn=$1 rep=$2 id="rbxport-$1-$2"
  if [ "$scn" = convert ]; then
    rbx_quit
    rm -rf "$RBX_CACHE"
  fi
  rbx_start
  if [ "$scn" = convert ]; then
    rbx_setting usbExport.maximumCompatibility true
    rbx_setting usbExport.conversionFormat '"mp3"'
  else
    rbx_setting usbExport.maximumCompatibility false
  fi
  [ "$scn" = resync ] || clear_stick
  audit_start "$id"
  local pid t0 t1 t2 st=0 c0 c1
  pid=$(pgrep -x rbxport | head -1 || true)
  [ -n "$pid" ] || die "rbxport is not running"
  c0=$("${LIB[@]}" cputime "$pid")
  rm -f "$OUT/rss-$id.json.stop"
  "${LIB[@]}" sample "$pid" "$OUT/rss-$id.json" &
  local sampler=$!
  t0=$(now)
  osascript -e 'tell application id "com.rbxport.app"' -e 'with timeout of 7200 seconds' \
    -e "export playlist \"$PLAYLIST\" to device \"$STICK\"" -e 'end timeout' -e 'end tell' > "$OUT/log-$id.txt" 2>&1 || st=$?
  t1=$(now); sync; t2=$(now)
  c1=$("${LIB[@]}" cputime "$pid")
  touch "$OUT/rss-$id.json.stop"; wait "$sampler" || true
  local outside stats rss_one rss_sum
  outside=$(audit_end "$id")
  stats=$("${LIB[@]}" stick "$VOL")
  rss_one=$(jget "$(cat "$OUT/rss-$id.json")" peak_rss_one_kb)
  rss_sum=$(jget "$(cat "$OUT/rss-$id.json")" peak_rss_sum_kb)
  row rbxport "$scn" "$rep" "$st" "$(sub "$t1" "$t0")" "$(sub "$t2" "$t0")" "$(sub "$c1" "$c0")" "$rss_one" "$rss_sum" "$outside" "$stats"
  echo "  rbxport $scn #$rep: exit $st, $(sub "$t1" "$t0") s, $outside file(s) written outside the stick"
}

cool() { echo "  cooling down ${COOL} s"; sleep "$COOL"; }

done_run() { [ -f "$CSV" ] && grep -q "^$1,$2,$3,0," "$CSV"; }   # tool scenario rep, recorded with exit 0
wanted() { case " ${TOOLS:-bakendeck rbxport} " in *" $1 "*) return 0 ;; esac; return 1; }

run() {   # resumable: a run already in runs.csv with exit 0 is skipped; TOOLS="rbxport" limits the tools
  local which="${1:-all}" rep tool
  check; environment
  for rep in $(seq "$REPS"); do
    local order=(bakendeck rbxport)
    [ $((rep % 2)) -eq 0 ] && order=(rbxport bakendeck)
    for tool in "${order[@]}"; do
      wanted "$tool" || continue
      if [ "$which" = all ] || [ "$which" = full ] || [ "$which" = resync ]; then
        if done_run "$tool" full "$rep" && done_run "$tool" resync "$rep"; then
          echo "  $tool full and resync #$rep: already done"
        else
          "run_${tool/bakendeck/baken}" full "$rep"; cool
          "run_${tool/bakendeck/baken}" resync "$rep"; cool
        fi
      fi
    done
    if [ "$which" = all ] || [ "$which" = convert ]; then
      for tool in "${order[@]}"; do
        wanted "$tool" || continue
        if done_run "$tool" convert "$rep"; then echo "  $tool convert #$rep: already done"; continue; fi
        "run_${tool/bakendeck/baken}" convert "$rep"; cool
      done
    fi
  done
  rbx_quit
  audit_start rbxport-quit-check; audit_end rbxport-quit-check > /dev/null
  "${LIB[@]}" report "$OUT"
}

first_launch() {
  check
  pgrep -xq rbxport && die "quit rbxport first"
  audit_start rbxport-first-launch
  echo "Open rbxport now. Go through its first-run screens with the default choices (leave Library Protection on),"
  echo "wait until the library is listed, then quit rbxport with Command-Q and press Return here."
  read -r
  pgrep -xq rbxport && die "rbxport is still running"
  echo "$(audit_end rbxport-first-launch) file(s) written outside the stick by the first launch; see $OUT/diff-rbxport-first-launch.txt"
}

case "${1:-}" in
  check) check ;;
  first-launch) first_launch ;;
  run) run "${2:-all}" ;;
  report) "${LIB[@]}" report "$OUT" ;;
  *) sed -n '2,17p' "$0"; exit 1 ;;
esac
