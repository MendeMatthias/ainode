// What may leave the Network view for the clipboard. Ids, hashes and links on
// a card are strings some host on the network sent; the engine passes them
// on as they came. A person pastes a copied id or command into a terminal, so
// a string is offered for copying only if it has exactly the form upstream
// gives it. Anything else is not copied and the card says so.
//
// Forms (upstream v0.34.16): a release id, a bounty id and a signer's id are
// 48-byte digests, 96 lowercase hex characters; a campaign's key hash is a
// SHA-256, 64 lowercase hex characters; a refund height is a block height
// below 500,000,000 (larger values are a time); a model link is btx:// plus
// 85 Bech32m characters, the same check as the backend's validate::model_uri.

/** A 48-byte digest as upstream writes it: release id, bounty id, signer id, model id. */
export function id96(v: unknown): string {
  return typeof v === "string" && /^[0-9a-f]{96}$/.test(v) ? v : "";
}

/** A SHA-256 as upstream writes it: a campaign's key hash. */
export function hash64(v: unknown): string {
  return typeof v === "string" && /^[0-9a-f]{64}$/.test(v) ? v : "";
}

/** A block height a refund can name: a whole number from 1 to 499,999,999. Else 0. */
export function blockHeight(v: unknown): number {
  const n = typeof v === "number" ? v : typeof v === "string" && /^\d{1,9}$/.test(v) ? Number(v) : NaN;
  return Number.isSafeInteger(n) && n >= 1 && n < 500_000_000 ? n : 0;
}

const CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const GEN = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];

function polymod(values: number[]): number {
  let chk = 1;
  for (const v of values) {
    const top = chk >>> 25;
    chk = (((chk & 0x01ffffff) << 5) ^ v) >>> 0;
    for (let i = 0; i < 5; i++) if ((top >>> i) & 1) chk = (chk ^ GEN[i]) >>> 0;
  }
  return chk >>> 0;
}

/**
 * A btx:// model link exactly as the backend accepts it (Rust
 * validate::model_uri, the same steps): the btx:// prefix, 85 characters of
 * the Bech32 alphabet, a valid Bech32m checksum over hrp "btx", version 1,
 * kind 0 (a model, not a bounty or another record), a 48-byte digest with no
 * stray bit. No surrounding space, no trailing slash: this is the form that is
 * copied. Returns the link, or "" for anything else.
 */
export function modelLink(v: unknown): string {
  if (typeof v !== "string" || !v.startsWith("btx://")) return "";
  const token = v.slice(6);
  if (token.length !== 85) return "";
  const data: number[] = [];
  for (const ch of token) {
    const i = CHARSET.indexOf(ch);
    if (i < 0) return "";
    data.push(i);
  }
  const hrp = [..."btx"].map((c) => c.charCodeAt(0));
  const values = [...hrp.map((c) => c >> 5), 0, ...hrp.map((c) => c & 31), ...data];
  if (polymod(values) !== 0x2bc830a3) return "";
  const payload = data.slice(0, -6);
  if (payload[0] !== 1 || payload[1] !== 0) return "";
  const digest = payload.slice(2);
  if (digest.length !== 77 || (digest[76] & 1) !== 0) return "";
  return v;
}
