#!/usr/bin/env python3
"""Keeps CurseForge binaries out of git by syncing them against mods.lock.json.

  lock     rebuild mods.lock.json from the files on disk (pre-commit)
  adopt    like lock, but commits files that are not on CurseForge instead of failing
  restore  make the folders match mods.lock.json (post-merge / post-checkout)
  status   show differences between disk and mods.lock.json
  setup    enable the hooks for this clone, then restore

Set CF_DOWNLOAD_TOKEN to a CurseForge API key to resolve files that neither the
lockfile nor minecraftinstance.json know about. modsync ignores local *-dev.jar builds.
"""
import hashlib
import json
import os
import shutil
import struct
import subprocess
import sys
import time
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor

FOLDERS = ("mods", "resourcepacks", "shaderpacks", "datapacks")
EXTENSIONS = (".jar", ".zip", ".disabled")
LOCAL_BUILD_SUFFIX = "-dev.jar"
LOCKFILE = "mods.lock.json"
INSTANCE_JSON = "minecraftinstance.json"
CF_API = "https://api.curseforge.com/v1"
MINECRAFT_GAME_ID = 432
CACHE_MAX_AGE_DAYS = 60
USER_AGENT = "raven-modsync/1.0"


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


ROOT = git("rev-parse", "--show-toplevel").strip()
os.chdir(ROOT)
STATE_DIR = os.path.join(git("rev-parse", "--absolute-git-dir").strip(), "modsync")
CACHE_DIR = os.path.join(STATE_DIR, "cache")
HASH_CACHE = os.path.join(STATE_DIR, "hashes.json")


def tracked_files():
    return set(git("ls-files", "-z", "--", *FOLDERS).split("\0"))


def managed_files():
    tracked = tracked_files()
    found = []
    for folder in FOLDERS:
        if not os.path.isdir(folder):
            continue
        for name in os.listdir(folder):
            path = f"{folder}/{name}"
            if name.endswith(EXTENSIONS) and not name.endswith(LOCAL_BUILD_SUFFIX) and os.path.isfile(path) and path not in tracked:
                found.append(path)
    return found


class HashCache:
    def __init__(self):
        try:
            with open(HASH_CACHE, encoding="utf-8") as f:
                self.entries = json.load(f)
        except (OSError, ValueError):
            self.entries = {}
        self.dirty = False

    def sha1(self, path):
        st = os.stat(path)
        cached = self.entries.get(path)
        if cached and cached[0] == st.st_size and cached[1] == st.st_mtime_ns:
            return cached[2]
        digest = sha1_of(path)
        self.entries[path] = [st.st_size, st.st_mtime_ns, digest]
        self.dirty = True
        return digest

    def save(self):
        if self.dirty:
            os.makedirs(STATE_DIR, exist_ok=True)
            with open(HASH_CACHE, "w", encoding="utf-8") as f:
                json.dump(self.entries, f)


def sha1_of(path):
    h = hashlib.sha1()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def cf_fingerprint(path):
    with open(path, "rb") as f:
        data = f.read().translate(None, b"\t\n\r ")
    mask, m, n = 0xFFFFFFFF, 0x5BD1E995, len(data)
    h, n4 = (1 ^ n) & mask, n & ~3
    for (k,) in struct.iter_unpack("<I", data[:n4]):
        k = (k * m) & mask
        k ^= k >> 24
        k = (k * m) & mask
        h = ((h * m) & mask) ^ k
    tail = n & 3
    if tail:
        if tail == 3:
            h ^= data[n4 + 2] << 16
        if tail >= 2:
            h ^= data[n4 + 1] << 8
        h ^= data[n4]
        h = (h * m) & mask
    h ^= h >> 13
    h = (h * m) & mask
    return h ^ (h >> 15)


def cdn_url(file_id, file_name):
    return f"https://edge.forgecdn.net/files/{file_id // 1000}/{file_id % 1000}/{urllib.parse.quote(file_name)}"


def read_lock():
    try:
        with open(LOCKFILE, encoding="utf-8") as f:
            return json.load(f)
    except FileNotFoundError:
        return {"files": []}


def write_lock(lock):
    lines = ["{"]
    for key in ("minecraft", "loader"):
        if lock.get(key):
            lines.append(f"  {json.dumps(key)}: {json.dumps(lock[key])},")
    files = sorted(lock["files"], key=lambda e: e["path"].lower())
    lines.append('  "files": [')
    lines += [f"    {json.dumps(e, ensure_ascii=False)}{',' if i < len(files) - 1 else ''}" for i, e in enumerate(files)]
    lines += ["  ]", "}", ""]
    with open(LOCKFILE, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(lines))


def instance_metadata():
    try:
        with open(INSTANCE_JSON, encoding="utf-8-sig") as f:
            instance = json.load(f)
    except (OSError, ValueError):
        return {}, None, None
    by_sha1 = {}
    for addon in instance.get("installedAddons", []):
        installed = addon.get("installedFile") or {}
        sha1 = next((h["value"] for h in installed.get("hashes", []) if h.get("type") == 1), None)
        if sha1:
            by_sha1[sha1] = (addon["addonID"], installed["id"], installed.get("downloadUrl") or cdn_url(installed["id"], installed["fileName"]))
    loader = (instance.get("baseModLoader") or {}).get("name")
    return by_sha1, instance.get("gameVersion"), loader


def api_request(method, path, body=None):
    key = os.environ.get("CF_DOWNLOAD_TOKEN")
    if not key:
        return None
    request = urllib.request.Request(
        CF_API + path,
        data=json.dumps(body).encode() if body is not None else None,
        method=method,
        headers={"x-api-key": key, "Content-Type": "application/json", "User-Agent": USER_AGENT},
    )
    with urllib.request.urlopen(request, timeout=60) as response:
        return json.load(response)["data"]


def resolve_by_fingerprint(paths):
    fingerprints = {cf_fingerprint(p): p for p in paths}
    data = api_request("POST", f"/fingerprints/{MINECRAFT_GAME_ID}", {"fingerprints": list(fingerprints)})
    if data is None:
        return {}
    resolved = {}
    for match in data.get("exactMatches", []):
        file = match["file"]
        path = fingerprints.get(file.get("fileFingerprint"))
        if path:
            resolved[path] = (match["id"], file["id"], file.get("downloadUrl") or cdn_url(file["id"], file["fileName"]))
    return resolved


def build_lock():
    hashes = HashCache()
    old = read_lock()
    known = {e["sha1"]: (e["projectId"], e["fileId"], e["url"]) for e in old["files"]}
    from_instance, minecraft, loader = instance_metadata()
    known.update(from_instance)

    files, unknown = [], []
    for path in managed_files():
        sha1 = hashes.sha1(path)
        if sha1 in known:
            files.append((path, sha1, known[sha1]))
        else:
            unknown.append(path)
    if unknown:
        for path, meta in resolve_by_fingerprint(unknown).items():
            files.append((path, hashes.sha1(path), meta))
            unknown.remove(path)
    hashes.save()

    lock = {
        "minecraft": minecraft or old.get("minecraft"),
        "loader": loader or old.get("loader"),
        "files": [
            {"path": path, "projectId": meta[0], "fileId": meta[1], "sha1": sha1, "size": os.path.getsize(path), "url": meta[2]}
            for path, sha1, meta in files
        ],
    }
    return lock, unknown


def cmd_lock():
    lock, unknown = build_lock()
    if unknown:
        print("modsync: these files are not on CurseForge, so you must commit them directly (git add -f <path>) or remove them:", file=sys.stderr)
        for path in sorted(unknown):
            print(f"  {path}", file=sys.stderr)
        if not os.environ.get("CF_DOWNLOAD_TOKEN"):
            print("modsync: CF_DOWNLOAD_TOKEN is not set, so modsync skipped the CurseForge fingerprint lookup.", file=sys.stderr)
        return 1
    write_lock(lock)
    git("add", LOCKFILE)
    return 0


def cmd_adopt():
    lock, unknown = build_lock()
    for path in sorted(unknown):
        git("add", "-f", "--", path)
        print(f"modsync: not on CurseForge, committing directly: {path}")
    write_lock(lock)
    git("add", LOCKFILE)
    return 0


def cached_path(sha1):
    return os.path.join(CACHE_DIR, sha1)


def download(entry):
    target = cached_path(entry["sha1"])
    temp = target + ".part"
    try:
        request = urllib.request.Request(entry["url"], headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(request, timeout=120) as response, open(temp, "wb") as f:
            shutil.copyfileobj(response, f, 1 << 20)
    except Exception:
        if not shutil.which("curl"):
            raise
        subprocess.run(["curl", "-fsSL", "-A", USER_AGENT, "-o", temp, entry["url"]], check=True)
    if sha1_of(temp) != entry["sha1"]:
        os.remove(temp)
        raise ValueError(f"hash mismatch for {entry['url']}")
    os.replace(temp, target)


def sync_plan(lock, hashes):
    wanted = {e["path"]: e for e in lock["files"]}
    present = {path: hashes.sha1(path) for path in managed_files()}
    missing = [e for path, e in wanted.items() if present.get(path) != e["sha1"]]
    extra = [path for path, sha1 in present.items() if wanted.get(path, {}).get("sha1") != sha1]
    return missing, extra, present


def warn_loader_mismatch(lock):
    _, _, local_loader = instance_metadata()
    if local_loader and lock.get("loader") and local_loader != lock["loader"]:
        print(f"modsync: WARNING: lockfile expects {lock['loader']} but this profile uses {local_loader}; change it in the CurseForge app", file=sys.stderr)


def cmd_restore():
    lock = read_lock()
    warn_loader_mismatch(lock)
    hashes = HashCache()
    missing, extra, present = sync_plan(lock, hashes)
    if not missing and not extra:
        hashes.save()
        return 0
    os.makedirs(CACHE_DIR, exist_ok=True)

    for path in extra:
        sha1 = present[path]
        if os.path.exists(cached_path(sha1)):
            os.remove(path)
        else:
            os.replace(path, cached_path(sha1))
        hashes.entries.pop(path, None)
        print(f"modsync: removed {path}")

    to_download = {e["sha1"]: e for e in missing if not os.path.exists(cached_path(e["sha1"]))}
    failed = set()
    if to_download:
        print(f"modsync: downloading {len(to_download)} file(s)")

        def fetch(entry):
            try:
                download(entry)
            except Exception as e:
                failed.add(entry["sha1"])
                print(f"modsync: failed {entry['path']}: {e}", file=sys.stderr)

        with ThreadPoolExecutor(6) as pool:
            list(pool.map(fetch, to_download.values()))

    for entry in missing:
        if entry["sha1"] in failed:
            continue
        source = cached_path(entry["sha1"])
        shutil.copyfile(source, entry["path"])
        os.utime(source)
        print(f"modsync: placed {entry['path']}")

    expire_cache()
    hashes.save()
    if failed:
        print(f"modsync: {len(failed)} file(s) failed to download; run `python .githooks/modsync.py restore` to retry", file=sys.stderr)
        return 1
    return 0


def expire_cache():
    cutoff = time.time() - CACHE_MAX_AGE_DAYS * 86400
    for name in os.listdir(CACHE_DIR):
        path = os.path.join(CACHE_DIR, name)
        if os.path.getmtime(path) < cutoff:
            os.remove(path)


def cmd_status():
    hashes = HashCache()
    missing, extra, _ = sync_plan(read_lock(), hashes)
    hashes.save()
    for e in missing:
        print(f"missing  {e['path']}")
    for path in extra:
        print(f"unlocked {path}")
    if not missing and not extra:
        print("in sync")
    return 0


def cmd_setup():
    git("config", "core.hooksPath", ".githooks")
    python = sys.executable.replace("\\", "/")
    git("config", "filter.iris.clean", f'"{python}" .githooks/iris_clean.py')
    print("modsync: hooks and filters enabled")
    return cmd_restore()


COMMANDS = {"lock": cmd_lock, "adopt": cmd_adopt, "restore": cmd_restore, "status": cmd_status, "setup": cmd_setup}

if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in COMMANDS:
        print(__doc__)
        sys.exit(2)
    sys.exit(COMMANDS[sys.argv[1]]())
