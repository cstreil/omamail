"""The development install target must use the plugin host, not a package manager."""
from pathlib import Path
import os
import subprocess
import unittest


ROOT = Path(__file__).resolve().parents[1]


class PluginWorkflow(unittest.TestCase):
    def make(self, *args, target_dir=None):
        env = dict(os.environ)
        if target_dir is None:
            env.pop("CARGO_TARGET_DIR", None)
        else:
            env["CARGO_TARGET_DIR"] = str(target_dir)
        return subprocess.run(["make", "--no-print-directory", *args], cwd=ROOT, env=env,
                              capture_output=True, text=True, check=True)

    def test_install_delegates_to_plugin_installer(self):
        result = self.make("-n", "install")
        self.assertEqual(result.stdout.strip().splitlines(), [
            f'cargo build --locked --release --target-dir "{ROOT}/target" --bin omamail',
            'python3 scripts/backend-runtime.py install-local',
            'bash scripts/link-plugin.sh',
        ])

    def test_install_honors_a_machine_local_cargo_target(self):
        target = Path("/tmp/omamail-machine-local-target")
        result = self.make("-n", "install", target_dir=target)
        self.assertEqual(result.stdout.strip().splitlines(), [
            f'cargo build --locked --release --target-dir "{target}" --bin omamail',
            'python3 scripts/backend-runtime.py install-local',
            'bash scripts/link-plugin.sh',
        ])

    def test_install_plugin_does_not_build_or_install_backend(self):
        result = self.make("-n", "install-plugin")
        self.assertEqual(result.stdout.strip().splitlines(), [
            'python3 scripts/backend-runtime.py uninstall',
            'bash scripts/link-plugin.sh',
        ])


if __name__ == "__main__":
    unittest.main()
