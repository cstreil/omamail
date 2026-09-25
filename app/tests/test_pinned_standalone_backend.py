#!/usr/bin/env python3
"""Synthetic, offline checks for the released standalone backend selector."""

import hashlib
import io
import os
from pathlib import Path
import stat
import struct
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile


ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "app/scripts/pinned_standalone_backend.py"
TARGETS = {
    "macos-aarch64": ("omamail-app-macos-aarch64.tar.gz", "Omamail.app/Contents/MacOS/omamail"),
    "linux-x86_64": ("omamail-app-linux-x86_64.tar.gz", "omamail.app/bin/omamail"),
    "windows-x86_64": ("omamail-app-windows-x86_64.zip", "omamail/bin/omamail.exe"),
}


def executable(target):
    data = bytearray(512)
    if target == "linux-x86_64":
        data[:7] = b"\x7fELF\x02\x01\x01"
        data[16:24] = b"\x03\x00\x3e\x00\x01\x00\x00\x00"
    elif target == "macos-aarch64":
        data[:4] = b"\xcf\xfa\xed\xfe"
        data[4:8] = b"\x0c\x00\x00\x01"
        data[12:16] = b"\x02\x00\x00\x00"
    else:
        data[:2] = b"MZ"
        struct.pack_into("<I", data, 0x3c, 128)
        data[128:134] = b"PE\x00\x00\x64\x86"
        data[148:150] = b"\xf0\x00"
        data[152:154] = b"\x0b\x02"
    return bytes(data)


class PinnedBackendTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.checksums = self.root / "SHA256SUMS"
        self.output = self.root / "private-backend"

    def create(self, target, entries=None):
        asset, path = TARGETS[target]
        archive = self.root / asset
        if entries is None:
            entries = [(path, executable(target), "file")]
        if target == "windows-x86_64":
            with zipfile.ZipFile(archive, "w", compression=zipfile.ZIP_DEFLATED) as package:
                for name, contents, kind in entries:
                    info = zipfile.ZipInfo(name)
                    info.compress_type = zipfile.ZIP_DEFLATED
                    info.create_system = 3
                    info.external_attr = ((stat.S_IFLNK if kind == "link" else stat.S_IFREG) | 0o755) << 16
                    package.writestr(info, contents)
        else:
            with tarfile.open(archive, "w:gz") as package:
                for name, contents, kind in entries:
                    info = tarfile.TarInfo(name)
                    if kind == "link":
                        info.type = tarfile.SYMTYPE
                        info.linkname = "elsewhere"
                    elif kind == "hardlink":
                        info.type = tarfile.LNKTYPE
                        info.linkname = "elsewhere"
                    elif kind == "device":
                        info.type = tarfile.CHRTYPE
                    else:
                        info.size = len(contents)
                    package.addfile(info, io.BytesIO(contents) if kind == "file" else None)
        self.checksums.write_bytes(hashlib.sha256(archive.read_bytes()).hexdigest().encode() + b"  " + asset.encode() + b"\n")
        return archive

    def run_helper(self, target, archive):
        env = dict(os.environ, PYTHONDONTWRITEBYTECODE="1")
        return subprocess.run([sys.executable, str(SCRIPT), "--target", target,
                               "--archive", str(archive), "--checksums", str(self.checksums),
                               "--output", str(self.output)],
                              cwd=self.root, env=env, text=True, capture_output=True)

    def refuse(self, target, archive):
        result = self.run_helper(target, archive)
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertFalse(self.output.exists(), result.stderr)
        self.assertEqual(list(self.root.glob(".omamail-backend-*")), [])
        return result

    def test_all_three_platforms_extract_only_the_exact_backend(self):
        for target in TARGETS:
            with self.subTest(target=target):
                self.output.unlink(missing_ok=True)
                asset, path = TARGETS[target]
                archive = self.create(target, [(path, executable(target), "file"),
                                               ("other/ignored", b"unused", "file")])
                result = self.run_helper(target, archive)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), str(self.output))
                self.assertEqual(self.output.read_bytes(), executable(target))
                self.assertEqual(stat.S_IMODE(self.output.stat().st_mode), 0o700)
                self.assertFalse((self.root / "other").exists())

    def test_corrupt_archive_is_rejected_before_any_backend_write(self):
        target = "linux-x86_64"
        archive = self.create(target)
        archive.write_bytes(archive.read_bytes() + b"tampering")
        self.assertIn("SHA256 mismatch", self.refuse(target, archive).stderr)

    def test_wrong_asset_name_is_rejected(self):
        archive = self.create("linux-x86_64")
        renamed = archive.with_name("omamail-linux-x86_64.tar.gz")
        archive.rename(renamed)
        self.refuse("linux-x86_64", renamed)

    def test_checksum_records_reject_ambiguity_and_noncanonical_text(self):
        archive = self.create("linux-x86_64")
        good = self.checksums.read_bytes()
        for data in (good + good, good[:-1], good.replace(b"\n", b"\r\n"),
                     b"# comment\n" + good, good.replace(b"  ", b" *"),
                     good.replace(b"omamail-app-linux-x86_64.tar.gz", b"../elsewhere"),
                     b"x" * 65537):
            with self.subTest(data=data[:80]):
                self.checksums.write_bytes(data)
                self.refuse("linux-x86_64", archive)

    def test_missing_or_duplicate_backend_member_is_rejected(self):
        for target in TARGETS:
            with self.subTest(target=target):
                path = TARGETS[target][1]
                self.refuse(target, self.create(target, [("other/file", executable(target), "file")]))
                self.refuse(target, self.create(target, [(path, executable(target), "file"),
                                                        (path, executable(target), "file")]))

    def test_rejects_case_ambiguous_and_traversing_members(self):
        for target in TARGETS:
            with self.subTest(target=target):
                path = TARGETS[target][1]
                for attack in (path.swapcase(), "../escape", "/absolute", "folder/./part",
                               "folder//part", "folder/..\\escape", "C:/other"):
                    self.refuse(target, self.create(target, [(path, executable(target), "file"),
                                                             (attack, b"x", "file")]))

    def test_rejects_links_except_unused_macos_framework_symlinks(self):
        for target in TARGETS:
            with self.subTest(target=target):
                path = TARGETS[target][1]
                entries = [(path, executable(target), "file"),
                           ("unused/link", b"elsewhere", "link")]
                archive = self.create(target, entries)
                if target == "macos-aarch64":
                    result = self.run_helper(target, archive)
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(self.output.read_bytes(), executable(target))
                    self.output.unlink()
                else:
                    self.refuse(target, archive)
        for kind in ("link", "hardlink", "device"):
            with self.subTest(kind=kind):
                self.refuse("macos-aarch64", self.create("macos-aarch64", [
                    (TARGETS["macos-aarch64"][1], executable("macos-aarch64"), "file"),
                    ("Omamail.app/Contents/MacOS", b"", kind)]))
        self.refuse("linux-x86_64", self.create("linux-x86_64", [
            (TARGETS["linux-x86_64"][1], executable("linux-x86_64"), "file"),
            ("unused/hardlink", b"", "hardlink")]))

    def test_rejects_oversize_backend_before_reading(self):
        target = "linux-x86_64"
        archive = self.create(target)
        # A sparse tar entry advertises more than the 128 MiB backend ceiling;
        # the fixture uses only one sparse local temporary file, never SSHFS.
        with tempfile.TemporaryFile() as large:
            large.seek(128 * 1024 * 1024)
            large.write(b"x")
            large.seek(0)
            with tarfile.open(archive, "w:gz") as package:
                info = tarfile.TarInfo(TARGETS[target][1])
                info.size = 128 * 1024 * 1024 + 1
                package.addfile(info, large)
        self.checksums.write_bytes(hashlib.sha256(archive.read_bytes()).hexdigest().encode()
                                   + b"  " + archive.name.encode() + b"\n")
        self.assertIn("bounded regular file", self.refuse(target, archive).stderr)

    def test_rejects_wrong_architecture_and_format(self):
        for target in TARGETS:
            with self.subTest(target=target):
                path = TARGETS[target][1]
                self.refuse(target, self.create(target, [(path, b"not an executable", "file")]))
                other = next(name for name in TARGETS if name != target)
                self.refuse(target, self.create(target, [(path, executable(other), "file")]))

    def test_rejects_wrong_cpu_with_valid_format_magic(self):
        mutations = {
            "linux-x86_64": (18, b"\xb7\x00"),  # AArch64 ELF
            "macos-aarch64": (4, b"\x07\x00\x00\x01"),  # x86_64 Mach-O
            "windows-x86_64": (132, b"\x64\xaa"),  # ARM64 PE
        }
        for target, (offset, replacement) in mutations.items():
            with self.subTest(target=target):
                binary = bytearray(executable(target))
                binary[offset:offset + len(replacement)] = replacement
                self.refuse(target, self.create(target, [(TARGETS[target][1], bytes(binary), "file")]))

    def test_existing_output_remains_untouched(self):
        archive = self.create("linux-x86_64")
        self.output.write_bytes(b"prior backend")
        result = self.run_helper("linux-x86_64", archive)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(self.output.read_bytes(), b"prior backend")


if __name__ == "__main__":
    unittest.main()
