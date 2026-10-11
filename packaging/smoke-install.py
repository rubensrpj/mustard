#!/usr/bin/env python3
"""Exercise actual installed executables in a disposable project, without AI.

Run in ephemeral CI hosts after installing the package. A PATH-free request
proves that the gateway can use its packaged rg; no fake executable or skipped
search can satisfy this check. Never changes a user's Claude configuration.
"""
import argparse
import json
import os
import sqlite3
from pathlib import Path
import subprocess
import tarfile
import tempfile
import zipfile


def smoke(bin_dir, version, bootstrap_archive=None):
    bin_dir = Path(bin_dir).resolve()
    suffix = ".exe" if os.name == "nt" else ""
    binary = lambda name: str(bin_dir / (name + suffix))
    for name in ("mustard", "mustard-rt", "scan", "rtk", "rg"):
        if not Path(binary(name)).is_file():
            raise AssertionError("missing installed executable: " + name)
    for name in ("COPYING", "LICENSE-MIT", "UNLICENSE"):
        assert (bin_dir / ("ripgrep-" + name)).is_file(), "missing ripgrep license"
    if bootstrap_archive:
        # Byte equality catches omitted dependencies and accidentally using a
        # second build (notably a newer Linux ABI) for the plugin bootstrap.
        files = [name + suffix for name in ("mustard", "mustard-rt", "scan", "rtk", "rg")]
        files += ["ripgrep-" + name for name in ("COPYING", "LICENSE-MIT", "UNLICENSE")]
        if str(bootstrap_archive).endswith(".zip"):
            with zipfile.ZipFile(bootstrap_archive) as archive:
                for name in files:
                    assert archive.read(name) == (bin_dir / name).read_bytes(), "bootstrap differs: " + name
        else:
            with tarfile.open(bootstrap_archive, "r:gz") as archive:
                members = {member.name.removeprefix("./"): member for member in archive.getmembers()}
                for name in files:
                    member = members[name]
                    assert member.isfile(), "bootstrap member is not a file"
                    assert archive.extractfile(member).read() == (bin_dir / name).read_bytes(), "bootstrap differs: " + name
    with tempfile.TemporaryDirectory(prefix="mustard-install-") as folder:
        root = Path(folder)
        (root / "src").mkdir()
        (root / "src/sample.py").write_bytes(b"def quartz():\n    return 42\n")
        (root / "mustard.json").write_text('{"ai":{"fallback":false,"vectors":true}}')
        env = os.environ.copy()
        env["MUSTARD_CLAUDE_BIN"] = str(root / "no-host-session")
        env["MUSTARD_SPEND_DIR"] = str(root / "spend")
        env["CLAUDE_CONFIG_DIR"] = str(root / "isolated-claude")
        env["MUSTARD_RT_DELEGATED"] = "1"
        env["PATH"] = str(bin_dir) + os.pathsep + env.get("PATH", "")
        for name in ("TYPESAFE_API_KEY", "JEV_API_KEY", "ANTHROPIC_API_KEY", "CLAUDE_PLUGIN_ROOT", "CLAUDE_PROJECT_DIR"):
            env.pop(name, None)

        def run(args, child_env=None):
            result = subprocess.run(args, cwd=root, env=child_env or env, capture_output=True, text=True, timeout=120)
            if result.returncode:
                raise AssertionError(str(args[:3]) + " failed: " + result.stdout[-2000:] + result.stderr[-2000:])
            return result.stdout

        run(["git", "init", "-q"])
        for name in ("mustard", "mustard-rt"):
            assert version in run([binary(name), "--version"]), "installed version differs"
        run([binary("rtk"), "--version"])
        run([binary("rg"), "--version"])
        installed = json.loads(run([binary("mustard-rt"), "run", "upsert"]))
        assert installed["installation"]["scan"]["ok"] is True, "installation did not prepare its index"
        assert installed["installation"]["project_ready"] is True, "required project components were not installed"
        assert installed["installation"]["host_ready"] is False, "missing host was presented as ready"
        assert installed["installation"]["complete"] is False, "an incomplete host was presented as complete"
        settings = json.loads((root / ".claude/settings.local.json").read_text())
        assert any(h.get("command") == "rtk hook claude" for group in settings["hooks"]["PreToolUse"] for h in group["hooks"]), "RTK hook missing from installation"
        hook = {"hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": str(root), "tool_input": {"command": "git status --short"}}
        rewritten = subprocess.run([binary("rtk"), "hook", "claude"], input=json.dumps(hook), cwd=root, env=env, text=True, capture_output=True, timeout=10)
        assert "rtk git status" in json.loads(rewritten.stdout)["hookSpecificOutput"]["updatedInput"]["command"], "installed RTK hook did not rewrite"
        config = json.loads((root / "mustard.json").read_text())
        config.update(version="0.0.0", installationChoice="preserve")
        (root / "mustard.json").write_text(json.dumps(config))
        updated = json.loads(run([binary("mustard-rt"), "run", "upsert"]))
        assert updated["installation"]["scan"]["ok"] is True
        assert updated["installation"]["scan"]["read"] == 0, "an unchanged project was read again"
        config = json.loads((root / "mustard.json").read_text())
        assert config["version"] == version and config["installationChoice"] == "preserve", "update lost version or personal choices"
        assert config["ai"]["fallback"] is False, "installation enabled paid AI"
        assert (root / ".claude/mustard/installation.json").is_file(), "installation receipt missing"
        run([binary("mustard"), "init", "--yes"])
        receipt = json.loads((root / ".claude/mustard/installation.json").read_text())
        assert receipt["version"] == version and receipt["scan"]["ok"] is True, "CLI skipped native verification"
        assert receipt["host_ready"] is False and receipt["complete"] is False, "CLI hid the missing host"
        with sqlite3.connect(root / ".claude/grain.db") as db:
            assert db.execute("SELECT count(*) FROM code_vectors").fetchone()[0] > 0, "embedded model was not applied"
        assert not (root / ".claude/judgements/attempts.ndjson").exists(), "installation called a remote model"
        run([binary("scan"), "scan", str(root), "--out", str(root / ".claude/grain.db"), "--json"])
        assert (root / ".claude/grain.db").is_file(), "scan did not create its database"
        raw = [binary("mustard-rt"), "run", "search", "--root", str(root), "--raw", "--", "rg", "-n", "--with-filename", "quartz", "src"]
        expected = "src/sample.py:1:def quartz():\n"
        assert run(raw).replace("\\", "/") == expected, "native search result differs"
        isolated = dict(env, PATH="")
        assert run(raw, isolated).replace("\\", "/") == expected, "packaged rg unavailable without PATH"
        request = {"schema_version": 1, "request": {"tool": "Grep", "input": {"pattern": "quartz", "path": "src", "output_mode": "content"}, "intent": "Locate the quartz declaration", "purpose": "locate"}}
        report = json.loads(run([binary("mustard-rt"), "run", "search", "--root", str(root), "--request", json.dumps(request)], isolated))
        assert report["ok"] is True and "quartz" in json.dumps(report["result"])
        assert report["learning"]["status"] == "stored-current-source-facts", "discovery was not persisted"
        assert report["learning"]["new_facts"] + report["learning"]["reused_facts"] > 0, "no source facts were crossed"
        assert report["crossing_status"] == "enriched", "installed gateway did not cross the scan"
        panel = json.loads(run([binary("mustard-rt"), "run", "panel", "--root", str(root)]))
        assert "project" in panel and "specs" in panel, "panel projection is unavailable"
        assert not list((root / ".claude").rglob("*.html")), "installation/search published an unsolicited page"
    print("PASS: fresh install/update, local vectors, RTK hook, honest readiness, native/typed search without PATH, learning and panel; no paid AI or publication")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--bin-dir", required=True)
    parser.add_argument("--version", required=True)
    parser.add_argument("--bootstrap-archive")
    args = parser.parse_args()
    smoke(args.bin_dir, args.version, args.bootstrap_archive)
