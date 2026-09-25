#!/usr/bin/env python3
"""Run only the Linux plugin backend's release/pin/API policy regressions.

The inherited full release test file also exercises the disabled cross-platform
standalone publisher. Its backend-only methods remain useful to the Omarchy
plugin's published/released API gate without running desktop package tests.
"""

import sys
import unittest

from test_backend_release import ReleaseTests


BACKEND_POLICY_CASES = (
    "test_release_pr_gate_requires_published_version_before_merge",
    "test_api_inventory_change_requires_contract_and_publication",
    "test_unreleased_cases_follow_their_methods_and_a_release_folds_them",
    "test_response_contract_changes_require_revision_bump",
    "test_unreleased_error_message_expectations_preserve_published_contract",
    "test_api_revisions_and_pin_require_canonical_values",
)


def main():
    suite = unittest.TestSuite(ReleaseTests(name) for name in BACKEND_POLICY_CASES)
    result = unittest.TextTestRunner(verbosity=1).run(suite)
    return 0 if result.wasSuccessful() else 1


if __name__ == "__main__":
    sys.exit(main())
