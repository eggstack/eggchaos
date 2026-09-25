"""Typed failure surfaces for the eggchaos native control client."""

from __future__ import annotations


class EggchaosError(Exception):
    """A native HTTP error answered by the server.

    Carries the HTTP status plus the bounded native error category
    (`code`) and detail. Never carries the bearer token.
    """

    def __init__(self, status: int, code: str, detail: str) -> None:
        super().__init__(f"eggchaos error {status} [{code}]: {detail}")
        self.status = status
        self.code = code
        self.detail = detail


class EggchaosTransportError(Exception):
    """Connect/read/timeout failure reaching the admin endpoint.

    Distinct from :class:`EggchaosError`: the server never answered.
    """


class EggchaosContractError(ValueError):
    """A response used an unknown discriminator or shape.

    Raised when the server speaks a newer contract than this SDK
    understands (for example an unknown fault `type` tag).
    """
