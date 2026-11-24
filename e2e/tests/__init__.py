"""Register all Tier 1-5 E2E test modules."""

from e2e.tests import (  # noqa: F401
    test_tier1_cli_and_slice,
    test_tier1_verify_mcp_site_tools_docs,
    test_tier2_boundaries_cli_slice_verify_mcp,
    test_tier2_boundaries_site_tools_docs,
    test_tier3_cross_feature,
    test_tier4_workflows,
    test_tier5_adversarial,
)
