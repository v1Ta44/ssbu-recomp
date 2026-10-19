"""Build the independent native resource backend, without lifting or linking game code."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess

REPO = Path(__file__).resolve().parent.parent
MANIFEST = REPO / "tools" / "mod_backend" / "Cargo.toml"


def source_fingerprint(repo=REPO):
    source = Path(repo) / "tools" / "mod_backend"
    paths = [source / "Cargo.toml", source / "Cargo.lock", *sorted((source / "src").glob("*.rs"))]
    digest = hashlib.sha256()
    for path in paths:
        digest.update(path.relative_to(source).as_posix().encode())
        digest.update(b"\0")
        digest.update(path.read_bytes())
    return digest.hexdigest()


def collect_notices(output, env):
    result = subprocess.check_output(
        ["cargo", "metadata", "--locked", "--format-version", "1", "--filter-platform",
         "x86_64-pc-windows-msvc", "--manifest-path", str(MANIFEST)],
        cwd=REPO, env=env, encoding="utf-8",
    )
    metadata = json.loads(result)
    nodes = {node["id"]: node for node in metadata["resolve"]["nodes"]}
    reachable, pending = set(), [metadata["resolve"]["root"]]
    while pending:
        key = pending.pop()
        if key not in reachable:
            reachable.add(key)
            pending.extend(nodes[key]["dependencies"])
    directory = output / "mod_backend_licenses"
    directory.mkdir(exist_ok=True)
    notices = []
    for package in metadata["packages"]:
        if package["id"] not in reachable:
            continue
        source = Path(package["manifest_path"]).parent
        destination = directory / (package["name"] + "-" + package["version"])
        copied = []
        for path in source.iterdir():
            if path.is_file() and path.name.lower().startswith(("license", "copying", "notice")):
                destination.mkdir(exist_ok=True)
                shutil.copy2(path, destination / path.name)
                copied.append(path.name)
        notices.append({"name": package["name"], "version": package["version"],
                        "source": package["source"], "license": package["license"], "files": copied})
    shutil.copy2(REPO / "LICENSE", directory / "ssbu_mod_backend-GPL-3.0.txt")
    (directory / "dependencies.json").write_text(
        json.dumps(notices, indent=2, ensure_ascii=False), encoding="utf-8")


def build(output, debug=False, target_dir=None):
    if not shutil.which("cargo"):
        raise SystemExit("The native resource backend requires Rust/Cargo for x64 Windows (tested with Rust 1.91).")
    env = dict(os.environ, CARGO_NET_GIT_FETCH_WITH_CLI="true")
    target = Path(target_dir) if target_dir else REPO / "build" / "mod-adapt" / "cargo"
    command = ["cargo", "build", "--locked", "--manifest-path", str(MANIFEST), "--target-dir", str(target)]
    if not debug:
        command.append("--release")
    subprocess.run(command, cwd=REPO, env=env, check=True)
    output = Path(output)
    output.mkdir(parents=True, exist_ok=True)
    for name in ("ssbu_mod_backend.dll", "ssbu_mod_backend.pdb"):
        source = target / ("debug" if debug else "release") / name
        if source.exists():
            shutil.copy2(source, output / name)
    collect_notices(output, env)
    (output / "ssbu_mod_backend.build.json").write_text(json.dumps({
        "source_sha256": source_fingerprint(), "configuration": "debug" if debug else "release",
        "dll_sha256": hashlib.sha256((output / "ssbu_mod_backend.dll").read_bytes()).hexdigest(),
    }, indent=2), encoding="utf-8")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", default=str(REPO / "build" / "work"))
    parser.add_argument("--debug", action="store_true")
    parser.add_argument("--target-dir", help="Cargo build cache (default build/mod-adapt/cargo)")
    args = parser.parse_args()
    build(args.output, args.debug, args.target_dir)
