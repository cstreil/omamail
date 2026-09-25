#!/usr/bin/env python3
"""Fail closed if the Omarchy-plugin-only fork regains unsupported CI or releases."""

from pathlib import Path
import re
import unittest


ROOT = Path(__file__).resolve().parents[1]
CI = ROOT / ".github/workflows/ci.yml"
NATIVE = ROOT / ".github/workflows/native-credentials.yml"
RELEASE = ROOT / ".github/workflows/release.yml"
MAKEFILE = ROOT / "Makefile"


def job(source, name):
    match = re.search(rf"(?ms)^  {re.escape(name)}:\n(.*?)(?=^  [a-zA-Z0-9_-]+:\n|\Z)", source)
    if not match:
        raise AssertionError(f"missing required job {name}")
    return match.group(1)


class OmarchyForkCI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.ci = CI.read_text(encoding="utf-8")
        cls.native = NATIVE.read_text(encoding="utf-8")
        cls.release = RELEASE.read_text(encoding="utf-8")

    def test_only_linux_plugin_jobs_run_on_pull_requests(self):
        jobs = set(re.findall(r"(?m)^  ([a-zA-Z0-9_-]+):$", self.ci.split("jobs:\n", 1)[1]))
        self.assertEqual(jobs, {"portable-tests", "unreleased-api", "published-assets", "published-backend"})
        self.assertNotIn("macos-", self.ci)
        self.assertNotIn("windows-", self.ci)
        self.assertNotIn("--features standalone", self.ci)
        portable = job(self.ci, "portable-tests")
        self.assertIn("make test-rust test-js test-shell-portable", portable)
        self.assertIn("tests/test_fork_ci_scope.py", portable)
        self.assertNotIn("test_release_workflow.py", portable)
        unreleased = job(self.ci, "unreleased-api")
        self.assertIn("cargo build --locked --bin omamail", unreleased)
        self.assertIn("test_backend_api.py --binary target/debug/omamail", unreleased)
        published = job(self.ci, "published-assets")
        self.assertIn("check-api --published", published)
        self.assertIn("test_backend_api.py --binary probe/omamail", published)
        self.assertIn("--released", published)
        required = job(self.ci, "published-backend")
        self.assertIn("needs: [published-assets, unreleased-api]", required)
        self.assertIn("if: always()", required)
        self.assertIn("name: Published backend merge gate", required)
        for name in jobs - {"published-assets"}:
            self.assertIn("runs-on: ubuntu-latest", job(self.ci, name))
        self.assertIn("runner: ubuntu-24.04", published)
        self.assertIn("runner: ubuntu-24.04-arm", published)
        self.assertIn("runs-on: ${{ matrix.runner }}", published)

    def test_default_local_validation_excludes_desktop_and_publisher_tests(self):
        make = MAKEFILE.read_text(encoding="utf-8")
        self.assertIn("validate: test qml-check", make)
        for target in ("test-js", "test-shell-portable"):
            with self.subTest(target=target):
                block = re.search(rf"(?ms)^{target}:\n(.*?)(?=^[a-zA-Z0-9_-]+:|\Z)", make)
                self.assertIsNotNone(block)
                for legacy in ("node app/tests/", "tests/test_app_make.py", "tests/test_publish.sh",
                               "tests/test_publish_backend.py", "tests/test_release_source.sh",
                               "tests/test_backend_release.py", "app/tests/test_release_workflow.py"):
                    self.assertNotIn(legacy, block.group(1))
        portable = re.search(r"(?ms)^test-shell-portable:\n(.*?)(?=^[a-zA-Z0-9_-]+:|\Z)", make)
        self.assertIn("python3 tests/test_backend_api_policy.py", portable.group(1))
        self.assertIn("test-legacy-release: test-legacy-app", make)

    def test_native_credentials_use_the_linux_plugin_backend(self):
        credentials = job(self.native, "credentials")
        self.assertIn("runs-on: ubuntu-22.04", credentials)
        self.assertIn("cargo test --locked --lib credentials", credentials)
        self.assertIn("cargo test --locked --lib credentials_native -- --ignored --test-threads=1", credentials)
        self.assertIn("dbus-run-session", credentials)
        self.assertNotIn("--features standalone", credentials)
        self.assertNotIn("macos-", self.native)
        self.assertNotIn("windows-", self.native)

    def test_legacy_publisher_is_refused_before_any_checkout_or_secret(self):
        prepare = job(self.release, "prepare")
        prefix = prepare.split("- uses: actions/checkout@", 1)[0]
        self.assertIn("- name: Refuse legacy standalone release in Omarchy-only fork", prefix)
        self.assertIn("exit 1", prefix)
        self.assertNotIn("secrets.", prefix)
        for dependent in ("build", "app-macos-aarch64", "app-linux-x86_64", "app-windows-x86_64", "publish-and-pin"):
            with self.subTest(job=dependent):
                self.assertRegex(job(self.release, dependent), r"(?m)^\s+needs:\s*(?:prepare|\[[^\]]*\bprepare\b[^\]]*\])\s*$")


if __name__ == "__main__":
    unittest.main()
