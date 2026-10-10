"""A merge may reuse CI only when the same tested tree passed every OS."""

import copy
import importlib.util
import io
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest
import urllib.parse
import urllib.request
import zipfile


SPEC = importlib.util.spec_from_file_location("routing", Path(__file__).with_name("ci-validation-route.py"))
ROUTING = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(ROUTING)

MERGE, HEAD, BASE, TREE = (character * 40 for character in "abcd")
REPO = "example/mustard"
PREFIX = f"/repos/{REPO}"
PULLS = f"{PREFIX}/commits/{MERGE}/pulls?per_page=100"
COMMIT = f"{PREFIX}/git/commits/{MERGE}"
ORIGINAL = f"{PREFIX}/git/commits/{HEAD}"
RUNS = f"{PREFIX}/actions/workflows/ci.yml/runs?" + urllib.parse.urlencode({
    "event": "pull_request", "head_sha": HEAD, "per_page": 100,
})
JOBS = f"{PREFIX}/actions/runs/123/jobs?filter=latest&per_page=100"
ARTIFACTS = f"{PREFIX}/actions/runs/123/artifacts?per_page=100"
DOWNLOAD = f"{PREFIX}/actions/artifacts/456/zip"


def archive(**changes):
    proof = {"repository": REPO, "head": HEAD, "tree": TREE, "run_id": "123", "run_attempt": "1"}
    proof.update(changes)
    stream = io.BytesIO()
    with zipfile.ZipFile(stream, "w") as zipped:
        zipped.writestr("ci-tested-tree.json", json.dumps(proof))
    return stream.getvalue()


def fixture():
    return {
        PULLS: [{"number": 12, "merged_at": "2026-10-10T12:00:00Z", "merge_commit_sha": MERGE,
                 "base": {"ref": "dev"}, "head": {"sha": HEAD}}],
        COMMIT: {"parents": [{"sha": BASE}, {"sha": HEAD}], "tree": {"sha": TREE}},
        ORIGINAL: {"tree": {"sha": TREE}},
        RUNS: {"workflow_runs": [{"id": 123, "run_attempt": 1, "event": "pull_request",
                                  "head_sha": HEAD, "status": "completed", "conclusion": "success",
                                  "pull_requests": []}]},
        JOBS: {"jobs": [{"name": name, "status": "completed", "conclusion": "success"}
                        for name in ROUTING.REQUIRED_JOBS]},
        ARTIFACTS: {"artifacts": [{"id": 456, "name": "ci-tested-tree-123-1", "expired": False}]},
        DOWNLOAD: archive(),
    }


class ValidationRouteTests(unittest.TestCase):
    def evaluate(self, data, **changes):
        args = {"event": "push", "branch": "dev", "sha": MERGE, "repository": REPO}
        args.update(changes)
        calls = []

        def api(endpoint):
            calls.append(endpoint)
            value = data[endpoint]
            if isinstance(value, Exception):
                raise value
            return copy.deepcopy(value)

        result = ROUTING.route(**args, api=api)
        self.assertEqual(result["warm_cache"], not result["validate"])
        return result, calls

    def test_pr_and_manual_runs_always_validate_without_api(self):
        for event in ("pull_request", "workflow_dispatch"):
            with self.subTest(event=event):
                result, calls = self.evaluate({}, event=event)
                self.assertTrue(result["validate"])
                self.assertEqual(calls, [])

    def test_verified_normal_merge_only_warms_cache(self):
        result, calls = self.evaluate(fixture())
        self.assertFalse(result["validate"])
        self.assertIn("PR #12", result["reason"])
        self.assertIn(DOWNLOAD, calls)

    def test_main_merge_uses_the_same_proof(self):
        data = fixture()
        data[PULLS][0]["base"]["ref"] = "main"
        self.assertFalse(self.evaluate(data, branch="main")[0]["validate"])

    def test_direct_push_requires_validation(self):
        self.assertTrue(self.evaluate({PULLS: []})[0]["validate"])

    def test_open_pr_or_wrong_destination_cannot_prove_merge(self):
        for change in ({"merged_at": None}, {"base": {"ref": "main"}}, {"merge_commit_sha": BASE}):
            with self.subTest(change=change):
                data = fixture()
                data[PULLS][0].update(change)
                result, calls = self.evaluate(data)
                self.assertTrue(result["validate"])
                self.assertNotIn(COMMIT, calls)

    def test_squash_rebase_or_unrelated_parent_requires_validation(self):
        for parents in ([], [{"sha": HEAD}], [{"sha": BASE}, {"sha": BASE}]):
            with self.subTest(parents=parents):
                data = fixture()
                data[COMMIT]["parents"] = parents
                self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_changed_integration_requires_validation(self):
        data = fixture()
        data[COMMIT]["tree"]["sha"] = BASE
        self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_actual_pr_checkout_must_match_even_if_head_matches(self):
        data = fixture()
        data[DOWNLOAD] = archive(tree=BASE)
        self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_failed_pending_wrong_head_or_non_pr_run_cannot_be_reused(self):
        for change in ({"conclusion": "failure"}, {"status": "in_progress"},
                       {"head_sha": BASE}, {"event": "push"}):
            with self.subTest(change=change):
                data = fixture()
                data[RUNS]["workflow_runs"][0].update(change)
                self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_each_required_job_must_pass(self):
        for job in ROUTING.REQUIRED_JOBS:
            for conclusion in ("failure", "skipped", "cancelled"):
                with self.subTest(job=job, conclusion=conclusion):
                    data = fixture()
                    next(item for item in data[JOBS]["jobs"] if item["name"] == job)["conclusion"] = conclusion
                    self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_older_success_cannot_hide_a_later_failed_or_running_validation(self):
        for change in ({"conclusion": "failure"}, {"status": "in_progress", "conclusion": None}):
            with self.subTest(change=change):
                data = fixture()
                newest = {**data[RUNS]["workflow_runs"][0], "id": 124, **change}
                data[RUNS]["workflow_runs"].insert(0, newest)
                self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_cache_only_run_cannot_approve_its_own_validation(self):
        data = fixture()
        data[JOBS]["jobs"] = [{"name": "Cache (windows-latest)", "status": "completed", "conclusion": "success"}]
        self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_missing_expired_or_previous_attempt_artifact_requires_validation(self):
        for artifacts in ([], [{"id": 456, "name": "ci-tested-tree-123-1", "expired": True}],
                          [{"id": 456, "name": "ci-tested-tree-123-0", "expired": False}]):
            with self.subTest(artifacts=artifacts):
                data = fixture()
                data[ARTIFACTS]["artifacts"] = artifacts
                self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_proof_must_belong_to_the_same_repository_head_and_run_attempt(self):
        for change in ({"repository": "other/mustard"}, {"head": BASE},
                       {"run_id": "124"}, {"run_attempt": "2"}):
            with self.subTest(change=change):
                data = fixture()
                data[DOWNLOAD] = archive(**change)
                self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_api_failure_or_invalid_response_falls_back_to_tests(self):
        for endpoint in fixture():
            for invalid in (OSError("unavailable"), None, []):
                with self.subTest(endpoint=endpoint, invalid=invalid):
                    data = fixture()
                    data[endpoint] = invalid
                    self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_invalid_or_oversized_archive_requires_validation(self):
        for value in (b"not a zip", archive(tree="x" * 70000)):
            with self.subTest(size=len(value)):
                data = fixture()
                data[DOWNLOAD] = value
                self.assertTrue(self.evaluate(data)[0]["validate"])

    def test_unknown_branch_or_malformed_identity_requires_validation_without_api(self):
        for change in ({"branch": "feature"}, {"sha": "invalid"}, {"repository": "../example/mustard"}):
            with self.subTest(change=change):
                result, calls = self.evaluate({}, **change)
                self.assertTrue(result["validate"])
                self.assertEqual(calls, [])

    def test_signed_artifact_redirect_does_not_receive_api_token(self):
        request = urllib.request.Request("https://api.github.com/artifact", headers={"Authorization": "Bearer example"})
        redirected = ROUTING.ArtifactRedirect().redirect_request(
            request, None, 302, "Found", {}, "https://storage.example/artifact?signature=example",
        )
        self.assertIsNone(redirected.get_header("Authorization"))

    def test_cli_emits_safe_fallback_when_no_api_token_is_available(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "outputs"
            environment = {**os.environ, "GITHUB_EVENT_NAME": "push", "GITHUB_REF_NAME": "dev",
                           "GITHUB_SHA": MERGE, "GITHUB_REPOSITORY": REPO, "GH_TOKEN": "",
                           "GITHUB_OUTPUT": str(output)}
            process = subprocess.run(["python3", str(Path(ROUTING.__file__))], env=environment,
                                     text=True, capture_output=True, check=True)
            self.assertTrue(json.loads(process.stdout)["validate"])
            self.assertEqual(output.read_text(), "validate=true\nwarm_cache=false\n")

    def test_cli_records_the_actual_pr_checkout_for_later_proof(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "outputs"
            environment = {**os.environ, "GITHUB_EVENT_NAME": "pull_request", "GITHUB_REPOSITORY": REPO,
                           "GITHUB_RUN_ID": "123", "GITHUB_RUN_ATTEMPT": "2", "PR_HEAD_SHA": HEAD,
                           "RUNNER_TEMP": directory, "GITHUB_OUTPUT": str(output)}
            subprocess.run(["python3", str(Path(ROUTING.__file__))], env=environment,
                           text=True, capture_output=True, check=True)
            proof = json.loads((Path(directory) / "ci-tested-tree.json").read_text())
            actual_tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], text=True).strip()
            self.assertEqual(proof, {"repository": REPO, "head": HEAD, "tree": actual_tree,
                                     "run_id": "123", "run_attempt": "2"})
            self.assertEqual(output.read_text(), "validate=true\nwarm_cache=false\n")


if __name__ == "__main__":
    unittest.main()
