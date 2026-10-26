"""Release contracts; run with python3 -m unittest discover -s tools/release/tests."""

import importlib.util
import io
import json
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[3]
SPEC = importlib.util.spec_from_file_location("release", ROOT / "tools/release/release.py")
release = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(release)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "source"
        self.root.mkdir()
        for name in ("Cargo.toml", "Cargo.lock", "LICENSE"):
            shutil.copyfile(ROOT / name, self.root / name)
        for source in (ROOT / "crates").glob("*/Cargo.toml"):
            target = self.root / source.relative_to(ROOT)
            target.parent.mkdir(parents=True)
            shutil.copyfile(source, target)
        shutil.copytree(ROOT / "packaging", self.root / "packaging")
        (self.root / "assets/regions").mkdir(parents=True)
        (self.root / "assets/regions/example.ron").write_text("region fixture")
        (self.root / "assets/locales/en-US").mkdir(parents=True)
        (self.root / "assets/locales/en-US/ui.ftl").write_text("fixture = Test")
        self.version = release.metadata(self.root)["version"]

    def test_current_workspace_and_matching_tag(self):
        self.assertEqual(
            release.metadata(self.root, f"v{self.version}")["version"], self.version
        )

    def test_mismatched_or_malformed_tags_are_refused(self):
        for tag in ("v999.0.0", self.version, "v1.2", "v1.02.3", "v1.2.3-01", "v1.2.3\n"):
            with self.subTest(tag=tag), self.assertRaises(ValueError):
                release.metadata(self.root, tag)

    def test_one_bump_changes_both_package_versions(self):
        next_version = "12.3.4-rc.2+build.5"
        for file in [self.root / "Cargo.toml", self.root / "Cargo.lock"]:
            file.write_text(file.read_text().replace(
                f'version = "{self.version}"', f'version = "{next_version}"'
            ))
        data = release.metadata(self.root, f"v{next_version}")
        self.assertEqual(data["version"], next_version)
        self.assertEqual(data["deb_version"], "12.3.4~rc.2+build.5")
        self.assertEqual(data["prerelease"], "true")

    def test_crates_must_inherit_the_workspace_version(self):
        path = self.root / "crates/game/Cargo.toml"
        path.write_text(path.read_text().replace("version.workspace = true", 'version = "9.9.9"', 1))
        with self.assertRaisesRegex(ValueError, "inherit"):
            release.metadata(self.root)

    def test_stale_lockfile_is_refused(self):
        path = self.root / "Cargo.lock"
        path.write_text(path.read_text().replace(
            f'name = "game"\nversion = "{self.version}"', 'name = "game"\nversion = "9.9.9"'
        ))
        with self.assertRaisesRegex(ValueError, "Cargo.lock"):
            release.metadata(self.root)

    def test_flatpak_and_desktop_identity_and_icon_contract(self):
        manifest = json.loads((self.root / f"packaging/linux/{release.APP_ID}.json").read_text())
        self.assertEqual(manifest["app-id"], release.APP_ID)
        self.assertEqual(manifest["command"], "megawatt-mogul")
        self.assertNotIn("--share=network", manifest["finish-args"])
        self.assertFalse(any(arg.startswith("--filesystem=") for arg in manifest["finish-args"]))
        self.assertIn("--socket=wayland", manifest["finish-args"])
        self.assertIn("--socket=fallback-x11", manifest["finish-args"])
        self.assertIn("--socket=pulseaudio", manifest["finish-args"])
        # Crash reports name functions only if the packaged game keeps its symbol table.
        self.assertEqual(manifest["build-options"], {"strip": False, "no-debuginfo": True})
        profile = release.read_toml(self.root / "Cargo.toml")["profile"]["release"]
        self.assertIn(profile.get("strip", "debuginfo"), ("none", "debuginfo", False))
        # The desktop theme directory must describe the actual PNG dimensions; preserve alpha.
        png = (self.root / "packaging/linux/icon.png").read_bytes()
        self.assertEqual(png[:8], b"\x89PNG\r\n\x1a\n")
        self.assertEqual(struct.unpack(">II", png[16:24]), (512, 512))
        self.assertEqual(png[25], 6)  # RGBA
        window = (ROOT / "crates/game/src/main.rs").read_text()
        self.assertIn(f'name: Some("{release.APP_ID}".into())', window)

    def test_staged_packages_share_assets_icon_version_and_desktop_identity(self):
        binary = Path(self.temp.name) / "game"
        binary.write_bytes(b"test executable")
        for prefix in ("/usr", "/app"):
            with self.subTest(prefix=prefix):
                stage = Path(self.temp.name) / prefix[1:]
                release.stage(self.root, binary, stage, prefix, "2026-10-25")
                self.assertEqual((stage / "lib/megawatt-mogul/game").read_bytes(), binary.read_bytes())
                self.assertTrue((stage / "lib/megawatt-mogul/game").stat().st_mode & 0o111)
                self.assertTrue((stage / "share/megawatt-mogul/assets/regions/example.ron").is_file())
                self.assertTrue((stage / "share/megawatt-mogul/assets/locales/en-US/ui.ftl").is_file())
                icon = stage / f'share/icons/hicolor/512x512/apps/{release.APP_ID}.png'
                self.assertEqual(icon.read_bytes(), (self.root / "packaging/linux/icon.png").read_bytes())
                desktop = (stage / f"share/applications/{release.APP_ID}.desktop").read_text()
                self.assertIn("Exec=megawatt-mogul", desktop)
                self.assertIn(f"Icon={release.APP_ID}", desktop)
                appstream = ET.parse(stage / f"share/metainfo/{release.APP_ID}.metainfo.xml")
                self.assertEqual(appstream.find("id").text, release.APP_ID)
                self.assertEqual(appstream.find("releases/release").attrib["version"], self.version)
                launcher = (stage / "bin/megawatt-mogul").read_text()
                self.assertIn(f'{prefix}/lib/megawatt-mogul/game', launcher)
                self.assertNotIn("@PREFIX@", launcher)

    def test_launcher_works_from_unrelated_directory_and_passes_overrides(self):
        # A tiny executable records argv. No environment or current-directory mutation in tests.
        binary = Path(self.temp.name) / "game"
        binary.write_text('#!/usr/bin/env python3\nimport json, sys\nprint(json.dumps(sys.argv[1:]))\n')
        stage = Path(self.temp.name) / "install with spaces"
        release.stage(self.root, binary, stage, "/usr", "2026-10-25")
        launcher = stage / "bin/megawatt-mogul"
        launcher.write_text(launcher.read_text().replace("/usr/", f"{stage}/"))
        result = subprocess.run(
            [str(launcher), "--save-dir", "my saves", "--exit-after-frames", "2"],
            cwd=self.temp.name, check=True, text=True, capture_output=True,
        )
        argv = json.loads(result.stdout)
        self.assertEqual(argv[:2], ["--assets", f"{stage}/share/megawatt-mogul/assets"])
        self.assertEqual(argv[-4:], ["--save-dir", "my saves", "--exit-after-frames", "2"])
        self.assertIn("megawatt-mogul/saves", argv[3])
        self.assertIn("megawatt-mogul/logs", argv[5])

    @unittest.skipUnless(
        sys.platform == "linux" and shutil.which("dpkg-shlibdeps") and shutil.which("dpkg-deb"),
        "Debian archive integration requires Linux and dpkg-dev",
    )
    def test_real_debian_archive_has_version_dependencies_icon_and_root_ownership(self):
        # A system ELF keeps this packaging test independent of the expensive game build.
        binary = Path(self.temp.name) / "game"
        shutil.copyfile(shutil.which("true"), binary)
        artifact = release.build_deb(
            self.root, binary, Path(self.temp.name) / "dist", "2026-10-25"
        )
        control = subprocess.run(
            ["dpkg-deb", "--field", str(artifact)], check=True, capture_output=True, text=True
        ).stdout
        self.assertIn(f"Version: {release.metadata(self.root)['deb_version']}\n", control)
        self.assertIn("Architecture: amd64\n", control)
        self.assertIn("libc6", control)
        self.assertIn("libxcursor1", control)
        self.assertIn("libvulkan1", control)
        archive = subprocess.run(
            ["dpkg-deb", "--fsys-tarfile", str(artifact)], check=True, capture_output=True
        ).stdout
        with tarfile.open(fileobj=io.BytesIO(archive)) as files:
            game = files.getmember("./usr/lib/megawatt-mogul/game")
            self.assertEqual((game.uid, game.gid, game.mode), (0, 0, 0o755))
            icon = files.extractfile(f"./usr/share/icons/hicolor/512x512/apps/{release.APP_ID}.png")
            self.assertEqual(icon.read(), (self.root / "packaging/linux/icon.png").read_bytes())
        self.assertIn("data.tar.xz", ar_members(artifact))


def ar_members(path):
    """Member names of an ar archive, such as a .deb."""
    data = path.read_bytes()
    names, offset = [], 8
    while offset + 60 <= len(data):
        header = data[offset:offset + 60]
        names.append(header[:16].decode().strip().rstrip("/"))
        size = int(header[48:58])
        offset += 60 + size + size % 2
    return names


if __name__ == "__main__":
    unittest.main()
