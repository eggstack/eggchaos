/** Typed failure surfaces for the eggchaos native control client. */

/** A native HTTP error answered by the server (status + bounded code/detail). */
export class EggchaosError extends Error {
  readonly status: number;
  readonly code: string;
  readonly detail: string;

  constructor(status: number, code: string, detail: string) {
    super(`eggchaos error ${status} [${code}]: ${detail}`);
    this.name = "EggchaosError";
    this.status = status;
    this.code = code;
    this.detail = detail;
  }
}

/** Connect/read/timeout failure: the server never answered. */
export class EggchaosTransportError extends Error {
  constructor(message: string, options?: { cause?: unknown }) {
    super(`transport failure: ${message}`, options);
    this.name = "EggchaosTransportError";
  }
}

/** A response used an unknown discriminator or shape (newer contract). */
export class EggchaosContractError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "EggchaosContractError";
  }
}
