# Toxiproxy v2.12 qualification corpus

The target oracle is Shopify Toxiproxy v2.12.0. Cases in this directory use
only the seven toxics present in that release. Timing cases use numeric
tolerance windows; ephemeral ports and timestamps may be normalized, but
status codes, toxic defaults, direction, and byte boundaries may not.

The local translation suite runs without an external oracle. The qualification
script reports the oracle as unavailable when the pinned v2.12.0 executable is
not installed, rather than treating source inspection as differential proof.
