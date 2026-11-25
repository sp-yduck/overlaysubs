#!/usr/bin/env python3
"""Fetch the speaker models that the installers bundle into src-tauri/speaker-models/.

The models are the `speaker` and `segmentation` entries of models.json (pinned URL, size, SHA-256). A file already
there with the right size and checksum is kept; anything else is downloaded again. Exits non-zero on a size or
checksum mismatch, so a changed upstream file fails the build instead of shipping.

    python3 scripts/stage-speaker-models.py
"""
import hashlib
import json
import os
import sys
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEST = os.path.join(ROOT, "src-tauri", "speaker-models")
KINDS = ("speaker", "segmentation")


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main():
    with open(os.path.join(ROOT, "models.json"), encoding="utf-8") as f:
        models = [m for m in json.load(f)["models"] if m["kind"] in KINDS]
    if sorted(m["kind"] for m in models) != sorted(KINDS):
        sys.exit("models.json must have exactly one speaker and one segmentation model")
    os.makedirs(DEST, exist_ok=True)
    for m in models:
        dst = os.path.join(DEST, m["file"])
        if os.path.exists(dst) and os.path.getsize(dst) == m["size"] and sha256(dst) == m["sha256"]:
            print(f"{m['file']}: already staged")
            continue
        part = dst + ".part"
        print(f"{m['file']}: downloading {m['url']}")
        req = urllib.request.Request(m["url"], headers={"User-Agent": "overlaysubs-build"})
        with urllib.request.urlopen(req, timeout=300) as r, open(part, "wb") as out:
            while chunk := r.read(1 << 20):
                out.write(chunk)
        size, digest = os.path.getsize(part), sha256(part)
        if size != m["size"] or digest != m["sha256"]:
            os.remove(part)
            sys.exit(f"{m['file']}: got {size} bytes, sha256 {digest}; models.json pins {m['size']} bytes, {m['sha256']}")
        os.replace(part, dst)
        print(f"{m['file']}: {size} bytes, sha256 ok")


if __name__ == "__main__":
    main()
