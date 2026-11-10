"""Build and smoke-test small, self-contained native release archives (stdlib only)."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    "x86_64-pc-windows-msvc": ("sectorline.exe", False),
    "x86_64-unknown-linux-gnu": ("sectorline", True),
    "aarch64-apple-darwin": ("sectorline", True),
}


def target_info(target):
    if target not in TARGETS:
        raise ValueError(f"Unverified target: {target}")
    return TARGETS[target]


def check_tag(tag, version):
    if tag and tag != f"v{version}":
        raise ValueError(f"Tag {tag!r} must match package version v{version}")


def archive_members(archive, expected):
    """Validate the exact payload before extracting an archive we just built."""
    if archive.suffix == ".zip":
        with zipfile.ZipFile(archive) as source:
            names = source.namelist()
            if len(names) != len(expected) or set(names) != set(expected):
                raise ValueError("Unexpected ZIP payload")
            return {name: source.read(name) for name in names}
    with tarfile.open(archive, "r:gz") as source:
        members = source.getmembers()
        if (len(members) != len(expected)
                or {m.name for m in members} != set(expected)
                or not all(m.isfile() for m in members)):
            raise ValueError("Unexpected tar payload")
        return {m.name: source.extractfile(m).read() for m in members}


def package(target, binary, output, version):
    binary_name, experimental = target_info(target)
    stem = f"sectorline-{version}-{target}" + ("-experimental" if experimental else "")
    output.mkdir(parents=True, exist_ok=True)
    archive = output / (stem + (".zip" if binary_name.endswith(".exe") else ".tar.gz"))
    if archive.exists():
        raise ValueError(f"Archive already exists: {archive}")
    revision = subprocess.check_output(
        ["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    payload = {
        binary_name: binary.read_bytes(),
        "README.md": (ROOT / "README.md").read_bytes(),
        "DATA.md": (ROOT / "DATA.md").read_bytes(),
        "CHANGELOG.md": (ROOT / "CHANGELOG.md").read_bytes(),
        "LICENSE": (ROOT / "LICENSE").read_bytes(),
        "THIRD-PARTY-LICENSES.txt": (ROOT / "THIRD-PARTY-LICENSES.txt").read_bytes(),
        "RELEASE-NOTES.md": (ROOT / ".github/RELEASE-NOTES.md").read_bytes(),
        "BUILD-INFO.json": (json.dumps({
            "version": version, "target": target, "source_commit": revision,
            "experimental": experimental,
            "binary_sha256": hashlib.sha256(binary.read_bytes()).hexdigest(),
            "data": "Embedded generated demo only; recordings downloaded separately",
        }, indent=2) + "\n").encode(),
    }
    expected = {f"{stem}/{name}": data for name, data in payload.items()}
    with tempfile.TemporaryDirectory(prefix="sectorline-package-") as temporary:
        staging = Path(temporary)
        for name, data in expected.items():
            path = staging / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            if path.name == binary_name:
                path.chmod(0o755)
        if archive.suffix == ".zip":
            with zipfile.ZipFile(archive, "x", compression=zipfile.ZIP_DEFLATED) as dest:
                for name in expected:
                    dest.write(staging / name, name)
        else:
            with tarfile.open(archive, "w:gz") as dest:
                for name in expected:
                    dest.add(staging / name, arcname=name, recursive=False)
        unpacked = staging / "unpacked"
        for name, data in archive_members(archive, expected).items():
            if data != expected[name]:
                raise ValueError(f"Archive content mismatch: {name}")
            path = unpacked / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
        executable = unpacked / stem / binary_name
        executable.chmod(0o755)
        fresh = staging / "empty-working-directory"
        fresh.mkdir()
        profile = staging / "unused-profile"
        env = dict(os.environ, PATH="", HOME=str(profile), USERPROFILE=str(profile),
                   LOCALAPPDATA=str(profile), XDG_CACHE_HOME=str(profile))
        for args, required in [(["--version"], version), (["--demo"], "20/20"),
                               (["--paths"], "cache")]:
            result = subprocess.run([str(executable), *args], cwd=fresh, env=env,
                                    capture_output=True, text=True, timeout=30, check=True)
            if required not in result.stdout:
                raise ValueError(f"Smoke output missing {required!r}: {result.stdout}")
        if profile.exists():
            raise ValueError("Demo smoke unexpectedly created a user profile/cache")
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest}  {archive.name}\n", encoding="utf-8")
    print(f"Verified archive payload and fresh-directory smoke: {archive.name}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check-version", action="store_true")
    parser.add_argument("--target")
    parser.add_argument("--binary", type=Path)
    parser.add_argument("--output", type=Path, default=ROOT / "dist")
    args = parser.parse_args()
    manifest = (ROOT / "f1_tui/Cargo.toml").read_text(encoding="utf-8")
    version = re.search(r'^version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"', manifest, re.M).group(1)
    check_tag(os.environ.get("RELEASE_TAG", ""), version)
    if args.check_version:
        print(f"Validated package version {version}")
        return
    if not args.target:
        parser.error("--target is required when packaging")
    binary_name, _ = target_info(args.target)
    binary = args.binary or ROOT / "target" / args.target / "release" / binary_name
    package(args.target, binary.resolve(), args.output.resolve(), version)


if __name__ == "__main__":
    main()
