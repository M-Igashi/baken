#!/usr/bin/env python3
"""End-to-end check of issue #154 on macOS 26, where ExFAT is mounted through FSKit.

Creates an ExFAT disk image, puts non-ASCII tracks on it the way Finder does (NFD names) and
the way other tools do (NFC names), then runs headroom, cdjsafe, rbsort and expressport on it
with the NFC paths rekordbox writes into collection.xml. Needs ffmpeg.

    python3 scripts/fskit-check.py [path/to/baken]   (default: target/debug/baken)

Exits non-zero if any check fails. Run it before tagging a release.
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
import unicodedata
import urllib.parse

BAKEN = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else "target/debug/baken")
NFC = lambda s: unicodedata.normalize("NFC", s)
NFD = lambda s: unicodedata.normalize("NFD", s)
failures = []


def check(ok, what):
    print(("  ok    " if ok else "  FAIL  ") + what)
    if not ok:
        failures.append(what)


def run(*args):
    r = subprocess.run([BAKEN, *args], capture_output=True, text=True)
    if r.returncode != 0:
        print("    " + (r.stdout + r.stderr).strip().replace("\n", "\n    "))
    return r.returncode == 0


def removable(path):
    subprocess.run(["rm", "-rf", path], capture_output=True)
    return not os.path.exists(path)


def settings_file(name):
    payload = {"DJMMYSETTING.DAT": 52, "DEVSETTING.DAT": 32}.get(name, 40)
    head = bytearray(104)
    head[0] = 0x60
    head[4:11] = b"PIONEER"
    head[36:46] = b"rekordbox\0"
    head[100:104] = payload.to_bytes(4, "little")
    body = bytes(head) + b"\1" * payload
    crc = 0
    for b in body if name == "DJMMYSETTING.DAT" else body[104:]:
        crc ^= b << 8
        for _ in range(8):
            crc = ((crc << 1) ^ 0x1021) & 0xFFFF if crc & 0x8000 else (crc << 1) & 0xFFFF
    return body + crc.to_bytes(2, "little") + b"\0\0"


def collection(tracks, playlist_ids):
    rows = "".join(
        f'<TRACK TrackID="{i}" Name="{NFC(t["title"])}" Artist="{NFC(t["artist"])}" Album="{NFC(t["album"])}" '
        f'Kind="FLAC File" Size="{os.path.getsize(t["path"])}" TotalTime="20" AverageBpm="120.00" '
        f'BitRate="1411" SampleRate="44100" Location="file://localhost{urllib.parse.quote(NFC(t["path"]))}">'
        '<TEMPO Inizio="0.000" Bpm="120.00" Metro="4/4" Battito="1"/></TRACK>'
        for i, t in enumerate(tracks, 1)
    )
    keys = "".join(f'<TRACK Key="{i}"/>' for i in playlist_ids)
    return (
        '<?xml version="1.0" encoding="UTF-8"?><DJ_PLAYLISTS Version="1.0.0">'
        '<PRODUCT Name="rekordbox" Version="7.2.18" Company="AlphaTheta"/>'
        f'<COLLECTION Entries="{len(tracks)}">{rows}</COLLECTION><PLAYLISTS>'
        '<NODE Type="0" Name="ROOT" Count="1">'
        f'<NODE Name="test" Type="1" KeyType="0" Entries="{len(playlist_ids)}">{keys}</NODE>'
        "</NODE></PLAYLISTS></DJ_PLAYLISTS>"
    )


def main():
    work = tempfile.mkdtemp(prefix="baken-fskit-")
    image = os.path.join(work, "fskit.dmg")
    subprocess.run(["hdiutil", "create", "-quiet", "-size", "200m", "-fs", "ExFAT", "-volname", "BAKENFSKIT", image], check=True)
    out = subprocess.run(["hdiutil", "attach", "-nobrowse", image], capture_output=True, text=True, check=True).stdout
    vol = re.search(r"(/Volumes/\S+)\s*$", out.strip()).group(1)
    try:
        mounted = subprocess.run(["mount"], capture_output=True, text=True).stdout
        if "fskit" not in next((l for l in mounted.splitlines() if vol in l), ""):
            print(f"warning: {vol} is not mounted through FSKit; this check means little here")

        album = os.path.join(vol, "lib", NFD("Rødhåd"), NFD("fabric presents Rødhåd"))
        os.makedirs(album)
        finder = {"title": "Hålla ä", "artist": "Rødhåd", "album": "fabric presents Rødhåd", "path": os.path.join(album, NFD("Hålla ä.flac"))}
        tool = {"title": "Wärme", "artist": "Rødhåd", "album": "fabric presents Rødhåd", "path": os.path.join(album, NFC("Wärme.flac"))}
        for t in (finder, tool):
            subprocess.run(["ffmpeg", "-v", "error", "-f", "lavfi", "-i", "sine=frequency=440:duration=20",
                            "-af", "volume=-20dB", "-ac", "2", "-ar", "44100", t["path"]], check=True)

        print("headroom, with the XML's NFC paths")
        backup = os.path.join(vol, "backup")
        check(run("headroom", NFC(finder["path"]), NFC(tool["path"]), "--backup", backup, "--no-report", "--no-update-check"), "exits cleanly")
        check(removable(backup), "rm -rf removes the backups")

        xml = os.path.join(vol, NFD("Sammlung ä.xml"))
        with open(xml, "w") as f:
            f.write(collection([finder, tool], [1, 2]))

        print("rbsort, in place on an XML Finder named")
        check(run("rbsort", NFC(xml)), "exits cleanly")

        print("cdjsafe")
        cdj = os.path.join(vol, "cdjsafe")
        check(run("cdjsafe", NFC(xml), "--playlist", "test", "--out-dir", cdj, "-o", os.path.join(vol, "out.xml")), "exits cleanly")
        check(removable(cdj), "rm -rf removes the transcoded files")

        print("expressport")
        settings = os.path.join(work, "settings")
        os.makedirs(settings)
        for name in ("MYSETTING.DAT", "MYSETTING2.DAT", "DJMMYSETTING.DAT", "DEVSETTING.DAT"):
            with open(os.path.join(settings, name), "wb") as f:
                f.write(settings_file(name))
        stick = os.path.join(vol, "stick")
        os.makedirs(stick)
        args = ["expressport", NFC(xml), "--device", stick, "--playlist", "test", "--settings-dir", settings, "--generate-analysis"]
        check(run(*args), "first export exits cleanly")
        leftovers = [os.path.join(r, f) for r, _, fs in os.walk(stick) for f in fs if f.startswith("._")]
        check(not leftovers, f"no AppleDouble files left on the stick {[os.path.relpath(p, stick) for p in leftovers]}")
        with open(xml, "w") as f:
            f.write(collection([finder, tool], [1]))
        check(run(*args, "--prune"), "second export with --prune exits cleanly")
        audio = [NFC(f) for _, _, fs in os.walk(os.path.join(stick, "Contents")) for f in fs if not f.startswith("._")]
        check(audio == [NFC("Hålla ä.flac")], f"--prune keeps the track still in the playlist and removes the other ({audio})")

        print("afterwards")
        check(removable(os.path.join(vol, "lib")), "rm -rf removes the tracks headroom rewrote")
        check(removable(xml), "rm removes the XML rbsort rewrote")
    finally:
        subprocess.run(["hdiutil", "detach", "-quiet", vol])
        shutil.rmtree(work, ignore_errors=True)

    print("PASS" if not failures else f"FAIL: {len(failures)} check(s)")
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()
