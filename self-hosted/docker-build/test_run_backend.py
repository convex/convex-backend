"""Run with python3 self-hosted/docker-build/test_run_backend.py."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


class RunBackendTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.data = self.root / "data"
        self.tmp = self.data / "tmp"
        self.stale = self.tmp / ".tmp-stale" / "payload"
        self.stale.parent.mkdir(parents=True)
        self.stale.write_text("temporary")
        self.keep = self.tmp / "operator-file"
        self.keep.write_text("persistent")
        (self.root / "read_credentials.sh").write_text("")
        backend = self.root / "convex-local-backend"
        backend.write_text("#!/bin/bash\ntouch backend-started\n")
        backend.chmod(0o755)

    def run_backend(self, **overrides):
        # Isolate the fixture from all operator configuration and credentials.
        env = {"PATH": os.defpath, "DATA_DIR": str(self.data)}
        env.update(overrides)
        return subprocess.run(
            ["bash", str(Path(__file__).with_name("run_backend.sh"))],
            cwd=self.root, env=env, capture_output=True, text=True,
        )

    def assert_started(self, result):
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue((self.root / "backend-started").exists())
        self.assertTrue(self.keep.exists())

    def test_default_cleanup_and_repeat_start(self):
        self.assert_started(self.run_backend())
        self.assertFalse(self.stale.exists())
        self.assert_started(self.run_backend())

    def test_custom_tmpdir_is_preserved(self):
        self.assert_started(self.run_backend(TMPDIR=str(self.tmp)))
        self.assertTrue(self.stale.exists())

    def test_opt_out_is_preserved(self):
        self.assert_started(self.run_backend(CLEAN_TMPDIR_ON_START="0"))
        self.assertTrue(self.stale.exists())

    def test_persistent_paths_are_preserved(self):
        for key in ["STORAGE_DIR", "SQLITE_DB", "CREDENTIALS_DIR"]:
            with self.subTest(key=key):
                self.assert_started(self.run_backend(**{key: str(self.stale.parent if key != "SQLITE_DB" else self.stale)}))
                self.assertTrue(self.stale.exists())

    def test_symlinked_storage_is_preserved(self):
        link = self.root / "storage-link"
        link.symlink_to(self.stale.parent, target_is_directory=True)
        self.assert_started(self.run_backend(STORAGE_DIR=str(link)))
        self.assertTrue(self.stale.exists())

    def test_symlink_entry_does_not_delete_target(self):
        target = self.root / "external"
        target.mkdir()
        payload = target / "payload"
        payload.write_text("persistent")
        (self.tmp / ".tmp-link").symlink_to(target, target_is_directory=True)
        self.assert_started(self.run_backend())
        self.assertTrue(payload.exists())
        self.assertFalse((self.tmp / ".tmp-link").is_symlink())

    def test_cleanup_failure_warns_and_continues(self):
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        remove = bin_dir / "rm"
        remove.write_text("#!/bin/bash\nexit 1\n")
        remove.chmod(0o755)
        result = self.run_backend(PATH=str(bin_dir) + os.pathsep + os.defpath)
        self.assert_started(result)
        self.assertIn("Failed to remove 1 of 1", result.stderr)
        self.assertTrue(self.stale.exists())

    def test_path_resolution_failure_skips_cleanup(self):
        bin_dir = self.root / "bin"
        bin_dir.mkdir()
        realpath = bin_dir / "realpath"
        realpath.write_text(
            '#!/bin/bash\ncase "$*" in *storage*) exit 1;; esac\n'
            'exec /usr/bin/realpath "$@"\n'
        )
        realpath.chmod(0o755)
        result = self.run_backend(PATH=str(bin_dir) + os.pathsep + os.defpath)
        self.assert_started(result)
        self.assertIn("WARNING:", result.stderr)
        self.assertTrue(self.stale.exists())


if __name__ == "__main__":
    unittest.main()
