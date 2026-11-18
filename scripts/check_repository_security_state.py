#!/usr/bin/env python3
"""Read-only M064 GitHub settings readback; requires an authenticated `gh` CLI.

Run manually with repository-admin read access. This script is deliberately
not wired into pull-request CI and never changes GitHub state.
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from typing import Any


class ApiError(RuntimeError):
    """A failed GitHub API read, with credential-free CLI diagnostics."""


def gh_json(endpoint: str) -> Any:
    result = subprocess.run(
        ["gh", "api", endpoint],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode != 0:
        detail = (result.stderr or result.stdout).strip()
        raise ApiError(f"gh api {endpoint} failed: {detail or 'unknown error'}")
    if not result.stdout.strip():
        return None
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise ApiError(f"gh api {endpoint} returned invalid JSON") from error


def vulnerability_alerts_enabled(repo: str) -> bool:
    endpoint = f"repos/{repo}/vulnerability-alerts"
    try:
        gh_json(endpoint)
    except ApiError as error:
        if re.search(r"HTTP\s+404\b", str(error)):
            return False
        raise
    # GitHub returns 204 with no response body when alerts are enabled.
    return True


def latest_successful_run(
    repo: str, workflow_id: int, branch: str | None = None
) -> dict[str, Any] | None:
    query = "status=success&per_page=1"
    if branch is not None:
        query += f"&branch={branch}"
    payload = gh_json(
        f"repos/{repo}/actions/workflows/{workflow_id}/runs?{query}"
    )
    runs = payload.get("workflow_runs", []) if isinstance(payload, dict) else []
    return runs[0] if runs else None


def latest_workflow_run(
    repo: str, workflow_id: int, branch: str
) -> dict[str, Any] | None:
    payload = gh_json(
        f"repos/{repo}/actions/workflows/{workflow_id}/runs?branch={branch}&per_page=1"
    )
    runs = payload.get("workflow_runs", []) if isinstance(payload, dict) else []
    return runs[0] if runs else None


def workflow_job_names(repo: str, run: dict[str, Any] | None) -> list[str]:
    if run is None:
        return []
    payload = gh_json(f"repos/{repo}/actions/runs/{run['id']}/jobs?per_page=100")
    jobs = payload.get("jobs", []) if isinstance(payload, dict) else []
    return sorted({job["name"] for job in jobs if job.get("name")})


def read_snapshot(repo: str) -> dict[str, Any]:
    repository = gh_json(f"repos/{repo}")
    if not isinstance(repository, dict):
        raise ApiError("repository API response was not an object")

    workflow_payload = gh_json(f"repos/{repo}/actions/workflows?per_page=100")
    workflows = (
        workflow_payload.get("workflows", [])
        if isinstance(workflow_payload, dict)
        else []
    )
    by_path = {workflow.get("path"): workflow for workflow in workflows}

    ci = by_path.get(".github/workflows/ci.yml")
    dependency_review = by_path.get(".github/workflows/dependency-review.yml")
    codeql = by_path.get(".github/workflows/codeql.yml")
    branch = repository.get("default_branch", "main")

    ci_run = (
        latest_successful_run(repo, ci["id"], branch)
        if ci and ci.get("state") == "active"
        else None
    )
    dependency_review_run = (
        latest_successful_run(repo, dependency_review["id"])
        if dependency_review and dependency_review.get("state") == "active"
        else None
    )
    codeql_run = (
        latest_workflow_run(repo, codeql["id"], branch)
        if codeql and codeql.get("state") == "active"
        else None
    )

    open_alerts = gh_json(
        f"repos/{repo}/dependabot/alerts?state=open&per_page=100"
    )
    ruleset_summaries = gh_json(f"repos/{repo}/rulesets?per_page=100")
    rulesets = [
        gh_json(f"repos/{repo}/rulesets/{ruleset['id']}")
        for ruleset in (ruleset_summaries if isinstance(ruleset_summaries, list) else [])
    ]
    current_user = gh_json("user")

    return {
        "repo": repo,
        "default_branch": branch,
        "current_user": current_user,
        "security_and_analysis": repository.get("security_and_analysis") or {},
        "vulnerability_alerts_enabled": vulnerability_alerts_enabled(repo),
        "open_dependabot_alerts": open_alerts if isinstance(open_alerts, list) else [],
        "rulesets": rulesets if isinstance(rulesets, list) else [],
        "ci": {
            "active": bool(ci and ci.get("state") == "active"),
            "run": ci_run,
            "jobs": workflow_job_names(repo, ci_run),
        },
        "dependency_review": {
            "active": bool(
                dependency_review and dependency_review.get("state") == "active"
            ),
            "run": dependency_review_run,
            "jobs": workflow_job_names(repo, dependency_review_run),
        },
        "codeql": {
            "active": bool(codeql and codeql.get("state") == "active"),
            "run": codeql_run,
            "jobs": workflow_job_names(repo, codeql_run),
        },
    }


def matching_ruleset(
    rulesets: list[dict[str, Any]], target: str, ref_pattern: str
) -> dict[str, Any] | None:
    for ruleset in rulesets:
        refs = ruleset.get("conditions", {}).get("ref_name", {}).get("include", [])
        if (
            ruleset.get("target") == target
            and ruleset.get("enforcement") == "active"
            and ref_pattern in refs
        ):
            return ruleset
    return None


def validate_snapshot(snapshot: dict[str, Any]) -> list[str]:
    errors: list[str] = []
    security = snapshot.get("security_and_analysis", {})
    expected_settings = {
        "secret_scanning": "Secret scanning",
        "secret_scanning_push_protection": "Secret scanning push protection",
        "secret_scanning_non_provider_patterns": "Secret scanning non-provider patterns",
        "secret_scanning_validity_checks": "Secret scanning validity checks",
        "dependabot_security_updates": "Dependabot security updates",
    }
    for key, label in expected_settings.items():
        status = security.get(key, {}).get("status")
        if status != "enabled":
            errors.append(f"{label} is {status or 'not reported'}, not enabled")
    if not snapshot.get("vulnerability_alerts_enabled"):
        errors.append("Dependabot vulnerability alerts are disabled")
    for alert in snapshot.get("open_dependabot_alerts", []):
        advisory = alert.get("security_advisory", {})
        dependency = alert.get("dependency", {}).get("package", {}).get("name", "?")
        errors.append(
            "Open Dependabot alert "
            f"#{alert.get('number', '?')} {advisory.get('ghsa_id', '?')} "
            f"for {dependency}; triage against its manifest lockfile"
        )

    user_id = snapshot.get("current_user", {}).get("id")
    branch_ruleset = matching_ruleset(
        snapshot.get("rulesets", []), "branch", "refs/heads/main"
    )
    tag_ruleset = matching_ruleset(
        snapshot.get("rulesets", []), "tag", "refs/tags/v*.*.*"
    )
    if branch_ruleset is None:
        errors.append("No active ruleset applies to refs/heads/main")
    else:
        rules = branch_ruleset.get("rules", [])
        rule_by_type = {rule.get("type"): rule for rule in rules}
        required_types = {
            "deletion",
            "non_fast_forward",
            "required_linear_history",
            "pull_request",
            "required_status_checks",
        }
        missing_types = sorted(required_types - set(rule_by_type))
        if missing_types:
            errors.append(f"main ruleset missing rules: {', '.join(missing_types)}")
        pr_parameters = rule_by_type.get("pull_request", {}).get("parameters", {})
        if pr_parameters.get("required_review_thread_resolution") is not True:
            errors.append("main ruleset does not require resolved review conversations")
        if pr_parameters.get("required_approving_review_count") != 0:
            errors.append("main ruleset review count differs from documented solo-maintainer policy")
        bypass = branch_ruleset.get("bypass_actors", [])
        if not any(
            actor.get("actor_type") == "User"
            and actor.get("actor_id") == user_id
            and actor.get("bypass_mode") == "pull_request"
            for actor in bypass
        ):
            errors.append("main ruleset does not grant the authenticated user PR-only bypass")
        status_rule = rule_by_type.get("required_status_checks", {}).get("parameters", {})
        if status_rule.get("strict_required_status_checks_policy") is not True:
            errors.append("main ruleset does not require checks to be up to date")
        required_contexts = {
            check.get("context")
            for check in status_rule.get("required_status_checks", [])
        }
        expected_contexts = set(snapshot.get("ci", {}).get("jobs", []))
        expected_contexts.update(snapshot.get("dependency_review", {}).get("jobs", []))
        expected_contexts.update(snapshot.get("codeql", {}).get("jobs", []))
        missing_contexts = sorted(expected_contexts - required_contexts)
        if missing_contexts:
            errors.append(
                "main ruleset does not require observed check contexts: "
                + ", ".join(missing_contexts)
            )

    if tag_ruleset is None:
        errors.append("No active ruleset applies to refs/tags/v*.*.*")
    else:
        rule_types = {rule.get("type") for rule in tag_ruleset.get("rules", [])}
        missing = sorted({"creation", "update", "deletion"} - rule_types)
        if missing:
            errors.append(f"release tag ruleset missing rules: {', '.join(missing)}")
        bypass = tag_ruleset.get("bypass_actors", [])
        if not any(
            actor.get("actor_type") == "User"
            and actor.get("actor_id") == user_id
            and actor.get("bypass_mode") == "always"
            for actor in bypass
        ):
            errors.append("release tag ruleset lacks the authenticated release-owner bypass")

    for name in ("ci", "dependency_review", "codeql"):
        if not snapshot.get(name, {}).get("active"):
            errors.append(f"{name} workflow is not active on the repository")
        run = snapshot.get(name, {}).get("run")
        if not run:
            errors.append(f"{name} has no successful hosted run to read back")
        elif run.get("conclusion") != "success":
            errors.append(f"{name} latest hosted run is not successful")
        elif not run.get("head_sha"):
            errors.append(f"{name} latest successful run has no head SHA")

    return errors


def summary(snapshot: dict[str, Any]) -> dict[str, Any]:
    def brief_run(run: dict[str, Any] | None) -> dict[str, Any] | None:
        if not run:
            return None
        return {
            "id": run.get("id"),
            "event": run.get("event"),
            "head_sha": run.get("head_sha"),
            "conclusion": run.get("conclusion"),
            "html_url": run.get("html_url"),
        }

    return {
        "repo": snapshot.get("repo"),
        "default_branch": snapshot.get("default_branch"),
        "rulesets": [
            {
                "id": ruleset.get("id"),
                "name": ruleset.get("name"),
                "target": ruleset.get("target"),
                "enforcement": ruleset.get("enforcement"),
            }
            for ruleset in snapshot.get("rulesets", [])
        ],
        "security_and_analysis": snapshot.get("security_and_analysis"),
        "vulnerability_alerts_enabled": snapshot.get("vulnerability_alerts_enabled"),
        "open_dependabot_alerts": [
            {
                "number": alert.get("number"),
                "ghsa": alert.get("security_advisory", {}).get("ghsa_id"),
                "package": alert.get("dependency", {}).get("package", {}).get("name"),
                "manifest": alert.get("dependency", {}).get("manifest_path"),
            }
            for alert in snapshot.get("open_dependabot_alerts", [])
        ],
        "codeql": {
            "active": snapshot.get("codeql", {}).get("active"),
            "run": brief_run(snapshot.get("codeql", {}).get("run")),
            "jobs": snapshot.get("codeql", {}).get("jobs", []),
        },
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", default="eggstack/eggchaos", help="OWNER/REPO")
    args = parser.parse_args(argv)
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        parser.error("--repo must be OWNER/REPO")
    try:
        snapshot = read_snapshot(args.repo)
    except ApiError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 2
    print(json.dumps(summary(snapshot), indent=2, sort_keys=True))
    errors = validate_snapshot(snapshot)
    if errors:
        for error in errors:
            print(f"FAIL: {error}", file=sys.stderr)
        return 1
    print('{"repository_security_readback":"pass"}')
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
