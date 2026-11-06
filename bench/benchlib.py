#!/usr/bin/env python3
"""Helpers for bench.sh: file snapshots, snapshot diffs, stick statistics, process sampling, and the report.

  benchlib.py snapshot OUT ROOT...        path, size, mtime, sha256 of every file under the roots
  benchlib.py diff A B                    added, removed, and changed files between two snapshots
  benchlib.py stick VOL                   what an export left on the stick, as JSON
  benchlib.py sample PID OUT              sum and max RSS of PID and its descendants every 0.2 s until PID exits or OUT.stop exists
  benchlib.py cputime PID                 user+sys seconds of PID and its descendants, from ps
  benchlib.py rusage FILE                 wall, user, sys, max RSS from /usr/bin/time -l output, as JSON
  benchlib.py report DIR                  summary.md from DIR/runs.csv and the per-run diffs
"""
import csv, hashlib, json, os, pathlib, re, statistics, subprocess, sys, time

AUDIO = {".mp3", ".m4a", ".mp4", ".aac", ".flac", ".wav", ".aif", ".aiff"}


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def snapshot(out, roots):
    rows = []
    for root in roots:
        for dirpath, _, names in os.walk(root):
            for n in names:
                p = os.path.join(dirpath, n)
                try:
                    st = os.lstat(p)
                    digest = sha256(p) if os.path.isfile(p) and not os.path.islink(p) else "-"
                except OSError as e:
                    rows.append((p, "?", "?", f"error:{e.errno}"))
                    continue
                rows.append((p, str(st.st_size), f"{st.st_mtime:.6f}", digest))
    with open(out, "w") as f:
        for r in sorted(rows):
            f.write("\t".join(r) + "\n")


def load(path):
    snap = {}
    for line in open(path):
        p, size, mtime, digest = line.rstrip("\n").split("\t")
        snap[p] = (size, mtime, digest)
    return snap


def diff(a, b):
    a, b = load(a), load(b)
    added = sorted(set(b) - set(a))
    removed = sorted(set(a) - set(b))
    changed = sorted(p for p in set(a) & set(b) if a[p][2] != b[p][2])
    touched = sorted(p for p in set(a) & set(b) if a[p][2] == b[p][2] and a[p][1] != b[p][1])
    for label, items in (("added", added), ("removed", removed), ("changed", changed), ("mtime only", touched)):
        for p in items:
            print(f"{label}\t{p}")
    return len(added) + len(removed) + len(changed)


def stick(vol):
    vol = pathlib.Path(vol)
    audio = files = size = anlz = 0
    dbs = {}
    for p in vol.rglob("*"):
        if not p.is_file() or p.name.startswith("._") or ".Spotlight-V100" in p.parts or ".fseventsd" in p.parts:
            continue
        files += 1
        s = p.stat().st_size
        size += s
        if p.suffix.lower() in AUDIO and "Contents" in p.parts:
            audio += 1
        if "USBANLZ" in p.parts:
            anlz += 1
        if p.name in ("export.pdb", "exportExt.pdb", "exportLibrary.db"):
            dbs[str(p.relative_to(vol))] = s
    print(json.dumps({"files": files, "bytes": size, "audio": audio, "anlz": anlz, "dbs": dbs}))


def tree(pid):
    out = subprocess.run(["ps", "-axo", "pid=,ppid=,rss=,time="], capture_output=True, text=True).stdout
    procs = {}
    for line in out.splitlines():
        p, pp, rss, t = line.split(None, 3)
        procs[int(p)] = (int(pp), int(rss), t.strip())
    keep, todo = set(), [int(pid)]
    while todo:
        cur = todo.pop()
        if cur in procs and cur not in keep:
            keep.add(cur)
            todo += [p for p, v in procs.items() if v[0] == cur]
    return {p: procs[p] for p in keep}


def seconds(t):
    parts = [float(x) for x in t.replace("-", ":").split(":")]
    total = 0.0
    for x in parts:
        total = total * 60 + x
    return total


def sample(pid, out):
    peak_sum = peak_one = 0
    stop = pathlib.Path(out + ".stop")
    while not stop.exists():
        procs = tree(pid)
        if not procs:
            break
        rss = [v[1] for v in procs.values()]
        peak_sum, peak_one = max(peak_sum, sum(rss)), max(peak_one, max(rss))
        time.sleep(0.2)
    pathlib.Path(out).write_text(json.dumps({"peak_rss_sum_kb": peak_sum, "peak_rss_one_kb": peak_one}))


def cputime(pid):
    print(f"{sum(seconds(v[2]) for v in tree(pid).values()):.2f}")


def rusage(path):
    text = open(path, errors="replace").read()
    m = re.search(r"([\d.]+) real\s+([\d.]+) user\s+([\d.]+) sys", text)
    rss = re.search(r"(\d+)\s+maximum resident set size", text)
    print(json.dumps({"real": float(m.group(1)), "user": float(m.group(2)), "sys": float(m.group(3)),
                      "peak_rss_one_kb": int(rss.group(1)) // 1024 if rss else 0}))


def report(d):
    d = pathlib.Path(d)
    rows = list(csv.DictReader(open(d / "runs.csv")))
    env = (d / "environment.txt").read_text() if (d / "environment.txt").exists() else ""
    out = ["# USB export benchmark", "", "```", env.strip(), "```", ""]
    med = lambda xs: f"{statistics.median(xs):.1f}" if xs else "-"
    rng = lambda xs: f"{min(xs):.1f} to {max(xs):.1f}" if xs else "-"
    out += ["| Scenario | Tool | Runs | Wall s, median (range) | Wall incl. sync s | CPU s (user+sys) | Peak RSS MB (largest process) | Audio files | Stick MB | Files written outside the stick |",
            "|---|---|---|---|---|---|---|---|---|---|"]
    for scn in ("full", "resync", "convert"):
        for tool in ("bakendeck", "rbxport"):
            rs = [r for r in rows if r["scenario"] == scn and r["tool"] == tool and r["status"] == "0"]
            if not rs:
                continue
            f = lambda k: [float(r[k]) for r in rs if r[k] not in ("", "-")]
            outside = sorted({int(r["outside_writes"]) for r in rs})
            out.append(f"| {scn} | {tool} | {len(rs)} | {med(f('wall'))} ({rng(f('wall'))}) | {med(f('wall_sync'))} | {med(f('cpu'))} | "
                       f"{med([x / 1024 for x in f('rss_one_kb')])} | {rs[-1]['audio']} | {float(rs[-1]['stick_bytes']) / 1e6:.0f} | {', '.join(map(str, outside))} |")
    failed = [r for r in rows if r["status"] != "0"]
    if failed:
        out += ["", "Failed runs: " + ", ".join(f"{r['tool']} {r['scenario']} #{r['rep']} (exit {r['status']})" for r in failed)]
    out += ["", "## Writes outside the stick", ""]
    for p in sorted(d.glob("diff-*.txt")):
        lines = [l for l in p.read_text().splitlines() if l and not l.startswith("mtime only")]
        out.append(f"- `{p.name}`: {len(lines)} file(s)" + (":" if lines else ""))
        out += [f"  - {l}" for l in lines[:40]]
    for p in sorted(d.glob("newer-*.txt")):
        lines = [l for l in p.read_text().splitlines() if l]
        out.append(f"- `{p.name}` (anything in the home folder modified during the run): {len(lines)} file(s)")
        out += [f"  - {l}" for l in lines[:25]]
    (d / "summary.md").write_text("\n".join(out) + "\n")
    print(d / "summary.md")


if __name__ == "__main__":
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "snapshot":
        snapshot(args[0], [a for a in args[1:] if os.path.exists(a)])
    elif cmd == "diff":
        diff(*args)
    elif cmd == "stick":
        stick(*args)
    elif cmd == "sample":
        sample(*args)
    elif cmd == "cputime":
        cputime(*args)
    elif cmd == "rusage":
        rusage(*args)
    elif cmd == "report":
        report(*args)
