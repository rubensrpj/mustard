#!/usr/bin/env python3
"""Run the shipped bootstrap/installer with isolated native test executables.

No network, account changes or model calls. Windows executes cmd/PowerShell;
POSIX executes sh. The slow-download case requires more than a hook's budget.
"""
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest
import zipfile

ROOT = Path(__file__).resolve().parents[1]
WINDOWS = os.name == "nt"
SUFFIX = ".exe" if WINDOWS else ""
HELPER = r'''
use std::{env,fs,path::Path,process,thread,time::Duration};
fn main() {
    let args:Vec<_>=env::args().skip(1).collect();
    let exe=env::current_exe().unwrap();
    let name=exe.file_stem().unwrap().to_str().unwrap();
    if name=="claude" {
        if args==["plugin","list","--json"] {println!("{}",env::var("TEST_PLUGIN_LIST").unwrap());}
        else if args.get(1).map(String::as_str)==Some("enable") {process::exit(1);}
        else if args==["plugin","list"] {println!("mustard@mustard-local");}
        return;
    }
    if name!="curl" {println!("mustard-rt 1.2.3");return;}
    fs::write(env::var("TEST_CURL_LOG").unwrap(),args.join("\n")).unwrap();
    let value=|flag:&str|args[args.iter().position(|arg|arg==flag).unwrap()+1].clone();
    let max:u64=value("--max-time").parse().unwrap();
    let required:u64=env::var("TEST_DOWNLOAD_SECONDS").unwrap_or("0".into()).parse().unwrap();
    if max<required {process::exit(28);}
    thread::sleep(Duration::from_secs(required));
    fs::copy(env::var("TEST_ARCHIVE").unwrap(),Path::new(&value("-o"))).unwrap();
}
'''


class Bootstrap(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory(prefix="mustard-bootstrap-helper-")
        folder = Path(cls.build.name)
        source = folder / "helper.rs"
        source.write_text(HELPER, encoding="utf-8")
        cls.helper = folder / ("helper" + SUFFIX)
        subprocess.run(["rustc", str(source), "-o", str(cls.helper)], check=True, timeout=60)

    @classmethod
    def tearDownClass(cls):
        cls.build.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="mustard-bootstrap-space ")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.plugin = self.root / "plugin"
        self.bin = self.plugin / "bin"
        self.bin.mkdir(parents=True)
        manifest = self.plugin / ".claude-plugin"
        manifest.mkdir()
        (manifest / "plugin.json").write_text('{\n  "version": "1.2.3"\n}\n')
        for script in ("mustard-boot", "mustard-boot.cmd"):
            shutil.copy2(ROOT / "plugin/bin" / script, self.bin / script)
        tools = self.root / "tools"
        tools.mkdir()
        for name in ("curl", "claude"):
            shutil.copy2(self.helper, tools / (name + SUFFIX))
        self.archive = self.root / ("payload.zip" if WINDOWS else "payload.tar.gz")
        if WINDOWS:
            with zipfile.ZipFile(self.archive, "w") as archive:
                for name in ("mustard-rt", "mustard", "scan", "rtk", "rg"):
                    archive.write(self.helper, name + SUFFIX)
        else:
            with tarfile.open(self.archive, "w:gz") as archive:
                for name in ("mustard-rt", "mustard", "scan", "rtk", "rg"):
                    archive.add(self.helper, arcname=name)
        self.log = self.root / "curl.txt"
        self.env = dict(os.environ, PATH=str(tools) + os.pathsep + os.environ.get("PATH", ""),
                        TEST_ARCHIVE=str(self.archive), TEST_CURL_LOG=str(self.log),
                        MUSTARD_PLUGIN_STEP_DEESCALATED="1", MUSTARD_PLUGIN_STEP_TIMEOUT="120")
        self.env.pop("TEST_DOWNLOAD_SECONDS", None)
        self.env.pop("CLAUDE_PLUGIN_ROOT", None)

    def boot(self, *args):
        if WINDOWS:
            # Pass cmd its native command line unchanged. list2cmdline would
            # escape these nested quotes for a C argv parser, not cmd /s /c.
            command = ('"' + os.environ.get("ComSpec", "cmd.exe") + '" /d /s /c ""'
                       + str(self.bin / "mustard-boot.cmd") + '" ' + " ".join(args) + '"')
        else:
            command = ["sh", str(self.bin / "mustard-boot"), *args]
        return subprocess.run(command, cwd=self.root, env=self.env, capture_output=True,
                              text=True, encoding="utf-8", errors="backslashreplace", timeout=30)

    def test_installer_download_can_exceed_ten_seconds(self):
        self.env["TEST_DOWNLOAD_SECONDS"] = "11"
        result = self.boot("--version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("mustard-rt 1.2.3", result.stdout)
        self.assertEqual((self.bin / ".version").read_text(), "1.2.3")
        self.assertTrue(all((self.bin / (name + SUFFIX)).is_file()
                            for name in ("mustard-rt", "mustard", "scan", "rtk", "rg")))

    def test_hook_download_remains_short_and_unstamped_on_failure(self):
        self.env["TEST_DOWNLOAD_SECONDS"] = "11"
        result = self.boot("on", "SessionStart")
        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertIn("download failed", result.stderr)
        self.assertFalse((self.bin / ".version").exists())
        self.assertFalse((self.bin / ("mustard-rt" + SUFFIX)).exists())

    def test_tool_hook_does_not_download(self):
        result = self.boot("on", "PreToolUse")
        self.assertEqual(result.returncode, 1)
        self.assertFalse(self.log.exists())

    def test_current_install_does_not_download(self):
        shutil.copy2(self.helper, self.bin / ("mustard-rt" + SUFFIX))
        (self.bin / ".version").write_text("1.2.3")
        result = self.boot("--version")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(self.log.exists())

    def test_bad_archive_does_not_claim_success(self):
        self.archive.write_bytes(b"not an archive")
        result = self.boot("--version")
        self.assertEqual(result.returncode, 1)
        self.assertIn("could not unpack", result.stderr)
        self.assertFalse((self.bin / ".version").exists())

    def plugin_step(self, enabled):
        entries = [{"id": "unrelated@marketplace", "enabled": True},
                   {"id": "mustard@mustard-local", "enabled": enabled,
                    "version": "1.2.3", "installPath": str(self.plugin)}]
        self.env["TEST_PLUGIN_LIST"] = json.dumps(entries, indent=2)
        if WINDOWS:
            command = ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                       str(ROOT / "packaging/installer/plugin-step.ps1")]
        else:
            command = ["sh", str(ROOT / "packaging/installer/plugin-step.sh")]
        return subprocess.run(command, cwd=self.root, env=self.env, capture_output=True,
                              text=True, encoding="utf-8", errors="backslashreplace", timeout=30)

    def test_already_enabled_is_confirmed_without_warning(self):
        result = self.plugin_step(True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("aviso:", result.stdout + result.stderr)
        self.assertIn("1.2.3", result.stdout)

    def test_an_unrelated_enabled_plugin_does_not_hide_enable_failure(self):
        result = self.plugin_step(False)
        self.assertEqual(result.returncode, 0)
        self.assertIn("aviso:", result.stdout + result.stderr)

    def test_installer_reports_a_real_download_failure(self):
        self.archive.write_bytes(b"not an archive")
        result = self.plugin_step(True)
        self.assertEqual(result.returncode, 0)
        self.assertIn("aviso:", result.stdout + result.stderr)
        self.assertIn("primeira sess", result.stdout + result.stderr)
        self.assertFalse((self.bin / ".version").exists())


if __name__ == "__main__":
    unittest.main(verbosity=2)
