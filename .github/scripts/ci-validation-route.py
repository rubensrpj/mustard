#!/usr/bin/env python3
"""Reuse successful PR validation only for an unchanged, verified merge."""

import io
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.parse
import urllib.request
import zipfile


REQUIRED_JOBS = {
    f"{kind} ({platform}-latest)"
    for kind in ("Test", "Mods")
    for platform in ("ubuntu", "macos", "windows")
}


def decision(validate, reason):
    return {"validate": validate, "warm_cache": not validate, "reason": reason}


class ArtifactRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, *args, **kwargs):
        redirected = super().redirect_request(request, *args, **kwargs)
        if redirected:
            # Artifact downloads redirect to a signed storage URL. The
            # repository token belongs only on the GitHub API request.
            redirected.remove_header("Authorization")
        return redirected


def tested_tree(prefix, run, repository, api):
    """Read the checkout tree recorded by this exact successful CI attempt."""
    artifacts = api(f"{prefix}/actions/runs/{run['id']}/artifacts?per_page=100")
    name = f"ci-tested-tree-{run['id']}-{run['run_attempt']}"
    for artifact in artifacts.get("artifacts", []):
        if artifact.get("name") != name or artifact.get("expired"):
            continue
        archive = api(f"{prefix}/actions/artifacts/{artifact['id']}/zip")
        with zipfile.ZipFile(io.BytesIO(archive)) as zipped:
            info = zipped.getinfo("ci-tested-tree.json")
            if info.file_size > 65536:
                raise ValueError("Oversized validation proof")
            proof = json.loads(zipped.read(info))
        if (proof.get("repository") == repository and proof.get("run_id") == str(run['id'])
                and proof.get("run_attempt") == str(run['run_attempt'])
                and proof.get("head") == run["head_sha"]):
            return proof.get("tree")
    return None


def route(event, branch, sha, repository, api):
    if event != "push":
        return decision(True, "Pull requests and manual runs require full validation")
    if branch not in ("dev", "main") or not re.fullmatch(r"[a-f0-9]{40}", sha):
        return decision(True, "Unrecognized push; full validation required")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        return decision(True, "Unrecognized repository; full validation required")

    prefix = f"/repos/{repository}"
    try:
        pulls = api(f"{prefix}/commits/{sha}/pulls?per_page=100")
        merged = [
            pr for pr in pulls
            if pr.get("merged_at") and pr.get("merge_commit_sha") == sha
            and pr.get("base", {}).get("ref") == branch
        ]
        if not merged:
            return decision(True, "No merged PR proves this push; full validation required")
        commit = api(f"{prefix}/git/commits/{sha}")
        parents = commit.get("parents", [])
        for pr in merged:
            head = pr["head"]["sha"]
            if len(parents) != 2 or parents[1]["sha"] != head:
                continue
            original = api(f"{prefix}/git/commits/{head}")
            if original["tree"]["sha"] != commit["tree"]["sha"]:
                continue
            query = urllib.parse.urlencode({
                "event": "pull_request", "head_sha": head, "per_page": 100,
            })
            runs = api(f"{prefix}/actions/workflows/ci.yml/runs?{query}")
            matching = [run for run in runs.get("workflow_runs", [])
                        if run.get("event") == "pull_request" and run.get("head_sha") == head]
            # GitHub returns newest first. An older success must never hide a
            # more recent failed or still-running validation of the same head.
            for run in matching[:1]:
                if run.get("status") != "completed" or run.get("conclusion") != "success":
                    continue
                # Completed PRs can disappear from run.pull_requests. Bind the
                # proof to the actual merged head, unchanged tree and full jobs.
                jobs = api(f"{prefix}/actions/runs/{run['id']}/jobs?filter=latest&per_page=100")
                approved = {
                    job["name"] for job in jobs.get("jobs", [])
                    if job.get("status") == "completed" and job.get("conclusion") == "success"
                }
                # A PR checks its synthetic merge, which can differ from its
                # head. Require the actual tested checkout tree as well.
                if REQUIRED_JOBS <= approved and tested_tree(prefix, run, repository, api) == commit["tree"]["sha"]:
                    return decision(False, f"PR #{pr['number']} passed CI run {run['id']}; unchanged merge, cache only")
        return decision(True, "No complete validation of the unchanged merge; full validation required")
    except (OSError, ValueError, KeyError, TypeError, AttributeError, zipfile.BadZipFile):
        # An unavailable API, incomplete response or missing proof cannot turn
        # a direct push or a changed integration into a successful test result.
        return decision(True, "Validation proof unavailable; full validation required")


def main():
    token = os.environ.get("GH_TOKEN", "")
    api_root = os.environ.get("GITHUB_API_URL", "https://api.github.com").rstrip("/")

    opener = urllib.request.build_opener(ArtifactRedirect())

    def api(endpoint):
        if not token:
            raise ValueError("No API token")
        request = urllib.request.Request(api_root + endpoint, headers={
            "Accept": "application/vnd.github+json", "Authorization": f"Bearer {token}",
            "X-GitHub-Api-Version": "2022-11-28",
        })
        with opener.open(request, timeout=15) as response:
            body = response.read(65537)
            if len(body) > 65536:
                raise ValueError("Oversized validation response")
            return body if endpoint.endswith("/zip") else json.loads(body)

    result = route(os.environ.get("GITHUB_EVENT_NAME"), os.environ.get("GITHUB_REF_NAME"),
                   os.environ.get("GITHUB_SHA", ""), os.environ.get("GITHUB_REPOSITORY", ""), api)
    if os.environ.get("GITHUB_EVENT_NAME") == "pull_request":
        proof = {
            "repository": os.environ["GITHUB_REPOSITORY"],
            "run_id": os.environ["GITHUB_RUN_ID"],
            "run_attempt": os.environ["GITHUB_RUN_ATTEMPT"],
            "head": os.environ["PR_HEAD_SHA"],
            "tree": subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], text=True).strip(),
        }
        (Path(os.environ["RUNNER_TEMP"]) / "ci-tested-tree.json").write_text(json.dumps(proof), encoding="utf-8")
    output = os.environ.get("GITHUB_OUTPUT")
    if output:
        with open(output, "a", encoding="utf-8") as target:
            for key in ("validate", "warm_cache"):
                target.write(f"{key}={str(result[key]).lower()}\n")
    print(json.dumps(result))


if __name__ == "__main__":
    main()
