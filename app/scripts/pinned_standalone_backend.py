#!/usr/bin/env python3
"""Copy the pinned published standalone backend from its verified native app archive.

This does not run the backend or extract any other archive member. The caller must
supply a trusted pinned-release archive and SHA256SUMS from that same release; a digest
alone does not authenticate who published either file.
"""

import argparse
import hashlib
import os
from pathlib import Path
import re
import stat
import struct
import sys
import tarfile
import tempfile
import zipfile


TARGETS = {
    "macos-aarch64": ("omamail-app-macos-aarch64.tar.gz", "Omamail.app/Contents/MacOS/omamail"),
    "linux-x86_64": ("omamail-app-linux-x86_64.tar.gz", "omamail.app/bin/omamail"),
    "windows-x86_64": ("omamail-app-windows-x86_64.zip", "omamail/bin/omamail.exe"),
}
MAX_CHECKSUMS = 65536
MAX_ARCHIVE = 512 * 1024 * 1024
MAX_MEMBERS = 20000
MAX_MEMBER = 512 * 1024 * 1024
MAX_TOTAL = 2 * 1024 * 1024 * 1024
MAX_BACKEND = 128 * 1024 * 1024
RECORD = re.compile(rb"([0-9a-f]{64})  ([A-Za-z0-9][A-Za-z0-9._-]*)\n\Z")


class BackendError(ValueError):
    """A published archive or its requested destination failed validation."""


def regular_input(path, limit):
    flags = os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0) | getattr(os, "O_BINARY", 0)
    fd = os.open(path, flags)
    try:
        size = os.fstat(fd)
        if not stat.S_ISREG(size.st_mode) or not 0 < size.st_size <= limit:
            raise BackendError("input is not a bounded regular file")
        return os.fdopen(fd, "rb")
    except BaseException:
        os.close(fd)
        raise


def expected_digest(checksums, name):
    with regular_input(checksums, MAX_CHECKSUMS) as source:
        data = source.read(MAX_CHECKSUMS + 1)
    if not data or len(data) > MAX_CHECKSUMS or not data.endswith(b"\n"):
        raise BackendError("SHA256SUMS must be bounded LF-terminated records")
    entries = {}
    seen = set()
    for line in data.splitlines(keepends=True):
        match = RECORD.fullmatch(line)
        if match is None:
            raise BackendError("malformed SHA256SUMS record")
        asset = match[2].decode("ascii")
        if asset.casefold() in seen:
            raise BackendError("duplicate SHA256SUMS asset")
        seen.add(asset.casefold())
        entries[asset] = match[1].decode("ascii")
    if name not in entries:
        raise BackendError("requested asset is absent from SHA256SUMS")
    return entries[name]


def checked_name(name):
    if (not name or name.startswith("/") or "\\" in name or "\x00" in name
            or any(part in ("", ".", "..") or ":" in part for part in name.split("/"))):
        raise BackendError("unsafe archive member path")
    return name.casefold()


def checked_members(archive, member_name, is_zip, allow_unrelated_symlinks=False):
    found = None
    seen = set()
    total = 0
    for count, info in enumerate(archive, 1):
        if count > MAX_MEMBERS:
            raise BackendError("too many archive members")
        raw_name = info.filename if is_zip else info.name
        key = checked_name(raw_name.rstrip("/") if is_zip and info.is_dir() else raw_name)
        if key in seen:
            raise BackendError("duplicate or ambiguous archive member")
        seen.add(key)
        if is_zip:
            kind = stat.S_IFMT(info.external_attr >> 16)
            directory = info.is_dir()
            if info.flag_bits & 1 or kind not in (0, stat.S_IFDIR if directory else stat.S_IFREG):
                raise BackendError("unsafe ZIP member type or encryption")
            if info.compress_type not in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED):
                raise BackendError("unsupported ZIP compression")
            size = info.file_size
            if info.compress_size > MAX_ARCHIVE:
                raise BackendError("ZIP compressed member too large")
        else:
            directory = info.isdir()
            if info.issym() and allow_unrelated_symlinks:
                # Qt's macOS frameworks contain release symlinks. They are never
                # extracted or followed; a link at the backend or any ancestor
                # would nevertheless make that logical path ambiguous.
                if member_name.casefold() == key or member_name.casefold().startswith(key + "/"):
                    raise BackendError("backend path contains archive symlink")
            elif not directory and not info.isfile():
                raise BackendError("unsafe tar member type")
            size = info.size
        if directory and size:
            raise BackendError("directory has payload")
        if size < 0 or size > MAX_MEMBER:
            raise BackendError("archive member too large")
        total += size
        if total > MAX_TOTAL:
            raise BackendError("archive payload too large")
        if raw_name == member_name:
            if directory or not 0 < size <= MAX_BACKEND:
                raise BackendError("backend is not a bounded regular file")
            found = info
    if found is None:
        raise BackendError("backend member missing from app archive")
    return found


def validate_binary(data, target):
    if target == "linux-x86_64":
        valid = (len(data) >= 64 and data[:7] == b"\x7fELF\x02\x01\x01"
                 and data[7] in (0, 3) and data[16:18] in (b"\x02\x00", b"\x03\x00")
                 and data[18:20] == b"\x3e\x00" and data[20:24] == b"\x01\x00\x00\x00")
    elif target == "macos-aarch64":
        valid = (len(data) >= 32 and data[:4] == b"\xcf\xfa\xed\xfe"
                 and data[4:8] == b"\x0c\x00\x00\x01" and data[12:16] == b"\x02\x00\x00\x00")
    else:
        valid = False
        if len(data) >= 0x40 and data[:2] == b"MZ":
            offset = struct.unpack_from("<I", data, 0x3c)[0]
            if offset <= len(data) - 26:
                optional_size = struct.unpack_from("<H", data, offset + 20)[0]
                valid = (data[offset:offset + 4] == b"PE\0\0"
                         and data[offset + 4:offset + 6] == b"\x64\x86"
                         and optional_size >= 2 and offset + 24 + optional_size <= len(data)
                         and data[offset + 24:offset + 26] == b"\x0b\x02")
    if not valid:
        raise BackendError(f"backend is not a {target} executable")


def verified_backend(target, archive_path, checksums):
    asset, member_name = TARGETS[target]
    if Path(archive_path).name != asset:
        raise BackendError(f"expected asset {asset}")
    wanted = expected_digest(checksums, asset)
    with regular_input(archive_path, MAX_ARCHIVE) as source:
        digest = hashlib.sha256()
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
        if digest.hexdigest() != wanted:
            raise BackendError("archive SHA256 mismatch")
        source.seek(0)
        try:
            if target == "windows-x86_64":
                with zipfile.ZipFile(source) as package:
                    member = checked_members(package.infolist(), member_name, True)
                    with package.open(member) as binary:
                        data = binary.read(MAX_BACKEND + 1)
            else:
                with tarfile.open(fileobj=source, mode="r:gz") as package:
                    member = checked_members(package, member_name, False,
                                             allow_unrelated_symlinks=(target == "macos-aarch64"))
                    with package.extractfile(member) as binary:
                        data = binary.read(MAX_BACKEND + 1)
        except (tarfile.TarError, zipfile.BadZipFile, EOFError, OSError, RuntimeError) as exc:
            raise BackendError(f"invalid app archive: {exc}") from exc
    expected_size = member.file_size if target == "windows-x86_64" else member.size
    if len(data) != expected_size:
        raise BackendError("backend payload size mismatch")
    validate_binary(data, target)
    return data


def write_backend(output, data):
    destination = Path(output)
    if destination.exists() or destination.is_symlink():
        raise BackendError("output must not already exist")
    if not destination.parent.is_dir():
        raise BackendError("output parent directory does not exist")
    tmp = None
    try:
        fd, tmp = tempfile.mkstemp(prefix=".omamail-backend-", dir=destination.parent)
        with os.fdopen(fd, "wb") as stream:
            if hasattr(os, "fchmod"):
                os.fchmod(stream.fileno(), 0o700)
            else:
                # Windows has no POSIX 0700 mode; its parent-directory ACL
                # must be private when the caller stages this CI artifact.
                os.chmod(tmp, 0o700)
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        if destination.exists() or destination.is_symlink():
            raise BackendError("output appeared during validation")
        os.replace(tmp, destination)
        tmp = None
    finally:
        if tmp is not None:
            os.unlink(tmp)
    return destination


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--target", required=True, choices=TARGETS)
    parser.add_argument("--archive", required=True, type=Path)
    parser.add_argument("--checksums", required=True, type=Path)
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    try:
        result = write_backend(args.output, verified_backend(args.target, args.archive, args.checksums))
    except (BackendError, OSError, ValueError) as exc:
        parser.exit(1, f"pinned standalone backend: {exc}\n")
    print(result)


if __name__ == "__main__":
    main()
