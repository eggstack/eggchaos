#!/usr/bin/env python3
"""Stdlib fixture tests for the manual M064 repository-settings readback."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import check_repository_security_state as checker


CI_JOBS = [
    "check (ubuntu-latest)",
    "check (macos-latest)",
    "check (windows-latest)",
    "performance-provenance",
    "api-gate",
    "python-native (ubuntu-latest, 3.12)",
    "python-native (macos-latest, 3.12)",
    "language-clients (macos-latest, 3.11, 20)",
]
DEPENDENCY_REVIEW_JOBS = ["review"]
CODEQL_JOBS = ["CodeQL (rust)", "CodeQL (actions)"]
ALL_CONTEXTS = CI_JOBS + DEPENDENCY_REVIEW_JOBS + CODEQL_JOBS


def fixture() -> dict:
    return {
        "repo": "eggstack/eggchaos",
        "default_branch": "main",
        "current_user": {"id": 262892073, "login": "dbowm91"},
        "security_and_analysis": {
            key: {"status": "enabled"}
            for key in (
                "secret_scanning",
                "secret_scanning_push_protection",
                "secret_scanning_non_provider_patterns",
                "secret_scanning_validity_checks",
                "dependabot_security_updates",
            )
        },
        "vulnerability_alerts_enabled": True,
        "open_dependabot_alerts": [],
        "rulesets": [
            {
                "id": 27505299,
                "name": "main-protection",
                "target": "branch",
                "enforcement": "active",
                "conditions": {"ref_name": {"include": ["refs/heads/main"]}},
                "bypass_actors": [
                    {
                        "actor_id": 262892073,
                        "actor_type": "User",
                        "bypass_mode": "pull_request",
                    }
                ],
                "rules": [
                    {"type": "deletion"},
                    {"type": "non_fast_forward"},
                    {"type": "required_linear_history"},
                    {
                        "type": "pull_request",
                        "parameters": {
                            "required_approving_review_count": 0,
                            "required_review_thread_resolution": True,
                        },
                    },
                    {
                        "type": "required_status_checks",
                        "parameters": {
                            "strict_required_status_checks_policy": True,
                            "required_status_checks": [
                                {"context": context, "integration_id": 15368}
                                for context in ALL_CONTEXTS
                            ],
                        },
                    },
                ],
            },
            {
                "id": 27505300,
                "name": "release-tag-protection",
                "target": "tag",
                "enforcement": "active",
                "conditions": {"ref_name": {"include": ["refs/tags/v*.*.*"]}},
                "bypass_actors": [
                    {
                        "actor_id": 262892073,
                        "actor_type": "User",
                        "bypass_mode": "always",
                    }
                ],
                "rules": [
                    {"type": "creation"},
                    {"type": "update"},
                    {"type": "deletion"},
                ],
            },
        ],
        "ci": {
            "active": True,
            "run": {"id": 1, "conclusion": "success", "head_sha": "ci-sha"},
            "jobs": CI_JOBS,
        },
        "dependency_review": {
            "active": True,
            "run": {
                "id": 2,
                "conclusion": "success",
                "head_sha": "review-sha",
            },
            "jobs": DEPENDENCY_REVIEW_JOBS,
        },
        "codeql": {
            "active": True,
            "run": {
                "id": 3,
                "head_sha": "candidate-sha",
                "conclusion": "success",
            },
            "jobs": CODEQL_JOBS,
        },
    }


class RepositorySecurityReadbackTests(unittest.TestCase):
    def test_complete_state_passes(self) -> None:
        self.assertEqual(checker.validate_snapshot(fixture()), [])

    def test_failed_latest_codeql_run_is_reported(self) -> None:
        state = fixture()
        state["codeql"]["run"]["conclusion"] = "failure"
        errors = checker.validate_snapshot(state)
        self.assertTrue(
            any("codeql latest hosted run is not successful" in error for error in errors),
            errors,
        )

    def test_disabled_security_setting_is_reported(self) -> None:
        state = fixture()
        state["security_and_analysis"]["secret_scanning_validity_checks"] = {
            "status": "disabled"
        }
        errors = checker.validate_snapshot(state)
        self.assertTrue(any("validity checks" in error for error in errors), errors)

    def test_open_alert_is_not_silently_accepted(self) -> None:
        state = fixture()
        state["open_dependabot_alerts"] = [
            {
                "number": 4,
                "security_advisory": {"ghsa_id": "GHSA-test-0000"},
                "dependency": {
                    "package": {"name": "example"},
                    "manifest_path": "Cargo.lock",
                },
            }
        ]
        errors = checker.validate_snapshot(state)
        self.assertTrue(any("#4 GHSA-test-0000" in error for error in errors), errors)

    def test_missing_observed_codeql_context_is_reported(self) -> None:
        state = fixture()
        state["rulesets"][0]["rules"][-1]["parameters"]["required_status_checks"] = [
            check
            for check in state["rulesets"][0]["rules"][-1]["parameters"][
                "required_status_checks"
            ]
            if check["context"] != "CodeQL (rust)"
        ]
        errors = checker.validate_snapshot(state)
        self.assertTrue(any("CodeQL (rust)" in error for error in errors), errors)

    def test_missing_or_unreconciled_ruleset_is_reported(self) -> None:
        state = fixture()
        state["rulesets"] = state["rulesets"][:1]
        errors = checker.validate_snapshot(state)
        self.assertTrue(any("release-tag-protection" in error or "refs/tags" in error for error in errors), errors)


if __name__ == "__main__":
    unittest.main()
