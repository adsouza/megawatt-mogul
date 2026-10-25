#!/usr/bin/env python3
"""Shared release metadata and Linux staging. Python 3.11+, standard library only."""

import argparse
from datetime import date
import json
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parents[2]
APP_ID = "io.github.adsouza.MegawattMogul"
PACKAGE = "megawatt-mogul"
# SemVer 2.0: numeric prerelease identifiers cannot have leading zeroes.
NUMBER = r"(?:0|[1-9][0-9]*)"
IDENTIFIER = rf"(?:{NUMBER}|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
SEMVER = re.compile(
    rf"(?P<core>{NUMBER}\.{NUMBER}\.{NUMBER})"
    rf"(?:-(?P<pre>{IDENTIFIER}(?:\.{IDENTIFIER})*))?"
    r"(?:\+(?P<build>[0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


def read_toml(path):
    with path.open("rb") as source:
        return tomllib.load(source)


def metadata(root=ROOT, tag=None):
    workspace = read_toml(root / "Cargo.toml")["workspace"]
    version = workspace["package"]["version"]
    match = SEMVER.fullmatch(version)
    if not match:
        raise ValueError(f"Workspace version must be SemVer: {version!r}")
    if tag is not None and tag != f"v{version}":
        raise ValueError(f"Tag {tag!r} must match workspace version v{version}")
    locked = read_toml(root / "Cargo.lock")["package"]
    for member in workspace["members"]:
        package = read_toml(root / member / "Cargo.toml")["package"]
        if package["version"] != {"workspace": True}:
            raise ValueError(f"{member} must inherit version.workspace = true")
        entries = [p for p in locked if p["name"] == package["name"] and "source" not in p]
        if len(entries) != 1 or entries[0]["version"] != version:
            raise ValueError(f"Cargo.lock version for {package['name']} must be {version}")
    deb_version = match["core"]
    if match["pre"]:
        deb_version += f"~{match['pre']}"
    if match["build"]:
        deb_version += f"+{match['build']}"
    return {
        "version": version,
        "deb_version": deb_version,
        "prerelease": "true" if match["pre"] else "false",
        "app_id": APP_ID,
    }


def write(path, text, executable=False):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.chmod(0o755 if executable else 0o644)


def copy(source, target, executable=False):
    target.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(source, target)
    target.chmod(0o755 if executable else 0o644)


def stage(root, binary, output, prefix, release_date):
    """Write the contents of a prefix, for both /usr and Flatpak's /app."""
    data = metadata(root)
    date.fromisoformat(release_date)
    source = root / "packaging/linux"
    copy(binary, output / f"lib/{PACKAGE}/game", executable=True)
    for asset in sorted((root / "assets").rglob("*")):
        if asset.is_file():
            copy(asset, output / f"share/{PACKAGE}/assets" / asset.relative_to(root / "assets"))
    write(output / f"bin/{PACKAGE}", (source / f"{PACKAGE}.sh").read_text().replace(
        "@PREFIX@", prefix
    ), executable=True)
    copy(source / f"{APP_ID}.desktop", output / f"share/applications/{APP_ID}.desktop")
    copy(source / "icon.png", output / f"share/icons/hicolor/512x512/apps/{APP_ID}.png")
    appstream = (source / f"{APP_ID}.metainfo.xml.in").read_text()
    write(output / f"share/metainfo/{APP_ID}.metainfo.xml", appstream.replace(
        "@VERSION@", data["version"]
    ).replace("@DATE@", release_date))
    copy(root / "LICENSE", output / f"share/doc/{PACKAGE}/copyright")


def build_deb(root, binary, output, release_date):
    """Compute ELF dependencies on the build host; add libraries loaded via dlopen."""
    data = metadata(root)
    output.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="megawatt-deb-") as temporary:
        work = Path(temporary)
        tree = work / "package"
        stage(root, binary, tree / "usr", "/usr", release_date)
        # dpkg-shlibdeps requires a source control file even when printing to stdout.
        write(work / "debian/control", f"Source: {PACKAGE}\n\nPackage: {PACKAGE}\nArchitecture: amd64\n")
        scanned = subprocess.run(
            ["dpkg-shlibdeps", "-O", f"-e{tree}/usr/lib/{PACKAGE}/game"],
            cwd=work, check=True, text=True, capture_output=True,
        ).stdout.strip()
        if not scanned.startswith("shlibs:Depends=") or "\n" in scanned:
            raise ValueError(f"Unexpected dpkg-shlibdeps output: {scanned!r}")
        dependencies = scanned.removeprefix("shlibs:Depends=").split(", ")
        dependencies += [
            "libx11-6", "libx11-xcb1", "libxcursor1", "libxi6", "libxcb1", "libxcb-render0", "libxcb-shape0",
            "libxcb-xfixes0", "libxkbcommon0", "libxkbcommon-x11-0", "libwayland-client0",
            "libwayland-cursor0", "libwayland-egl1", "libudev1", "libvulkan1", "libegl1",
        ]
        installed_kib = (sum(p.stat().st_size for p in tree.rglob("*") if p.is_file()) + 1023) // 1024
        write(tree / "DEBIAN/control", f"""Package: {PACKAGE}
Version: {data['deb_version']}
Section: games
Priority: optional
Architecture: amd64
Maintainer: Megawatt Mogul contributors <adsouza@users.noreply.github.com>
Homepage: https://github.com/adsouza/megawatt-mogul
Installed-Size: {installed_kib}
Depends: {', '.join(dict.fromkeys(dependencies))}
Recommends: mesa-vulkan-drivers
Description: Build a power company and keep the grid balanced
 A single-player electricity grid simulation on an interactive 3D tabletop.
""")
        artifact = output / f"{PACKAGE}_{data['deb_version']}_amd64.deb"
        # xz, as Flatpak's static deltas use LZMA: zstd made the same binary ~2 MB larger.
        subprocess.run(
            ["dpkg-deb", "--root-owner-group", "-Zxz", "--build", str(tree), str(artifact.resolve())],
            check=True,
        )
        return artifact


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    meta = commands.add_parser("metadata")
    meta.add_argument("--tag")
    meta.add_argument("--github-output", type=Path)
    for name in ("stage", "deb"):
        command = commands.add_parser(name)
        command.add_argument("--binary", type=Path, required=True)
        command.add_argument("--output", type=Path, required=True)
        command.add_argument("--date", required=True)
        if name == "stage":
            command.add_argument("--prefix", choices=("/usr", "/app"), required=True)
    args = parser.parse_args()
    try:
        if args.command == "metadata":
            data = metadata(tag=args.tag)
            if args.github_output:
                with args.github_output.open("a") as output:
                    for key, value in data.items():
                        output.write(f"{key}={value}\n")
            print(json.dumps(data))
        elif args.command == "stage":
            stage(ROOT, args.binary, args.output, args.prefix, args.date)
        else:
            print(build_deb(ROOT, args.binary.resolve(), args.output, args.date))
    except (ValueError, OSError, subprocess.CalledProcessError) as error:
        parser.exit(1, f"release: {error}\n")


if __name__ == "__main__":
    main()
