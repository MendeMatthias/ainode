import { describe, expect, it } from "vitest";
import { blockHeight, hash64, id96, modelLink } from "./safe";

// AInode's own four links (app/src-tauri/resources/catalog.json) and, from the
// captures of 2026-11-23, a test campaign's link and a bounty's link.
const CATALOG = [
  "btx://pq4admq4hc7nnvddga9z0flrvg8rhxld390gxdjae3nt8gywty205nn7yhruydsxdkx8k9h87c9uj3z3wcs9h",
  "btx://pqsltmv73ntyl6x7hf8kclk8fkcxnccsgfy30lmkuqqx0mc5uzhpd9uslhqz2s9x0874vky5yst7nl270yrxm",
  "btx://pq33nsv5hxj5mgrs4r4256h80g0y6n7uqhkv0j56cs3nrfq2c9jt686pm507apg2ygtvhd7uc43maj58gc8hu",
  "btx://pqcjx57rargtrp5us2cgvfdttffzxrfnwfwyrd3m63u9n8wtjqpvs2zf326d025uwrya6wy8tx69mxuxfntv9",
];
const CAMPAIGN_LINK = "btx://pq0yfnde3hy6jqhanm3zdml3wjzuz30yv5sxgaqm7amrr09psy8umn5t6phn3hjtr7lvv4dfzgxje07pvr2mg";
const BOUNTY_LINK = "btx://pqdrj326kp7h22u6djynfgsrvnn4mez7d53arpld2en37nqkgnnl3vg35zvgw3gx00mfxx9e5axm9h7kmuhks";

const HOSTILE = [
  "$(touch /tmp/x)", "`touch /tmp/x`", 'a" ; touch /tmp/x ; "', "a' ; touch /tmp/x ; '", "abc\ntouch /tmp/x", "abc\rtouch", "a".repeat(5000),
  "", " ", "3d2b0dfa", "${IFS}", "a;b", "a|b", "a&b", "a>b", "\\", "\u0000", "\u202e" + "a".repeat(95),
];

/** The same link with its kind changed to 1 and the checksum made right again: valid Bech32m, but not a model. */
function otherKind(link: string): string {
  const A = "qpzry9x8gf2tvdw0s3jn54khce6mua7l";
  const GEN = [0x3b6a57b2, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
  const polymod = (v: number[]) => {
    let chk = 1;
    for (const x of v) {
      const top = chk >>> 25;
      chk = (((chk & 0x01ffffff) << 5) ^ x) >>> 0;
      for (let i = 0; i < 5; i++) if ((top >>> i) & 1) chk = (chk ^ GEN[i]) >>> 0;
    }
    return chk >>> 0;
  };
  const data = [...link.slice(6)].map((c) => A.indexOf(c)).slice(0, -6);
  data[1] = 1;
  const hrp = [..."btx"].map((c) => c.charCodeAt(0));
  const pre = [...hrp.map((c) => c >> 5), 0, ...hrp.map((c) => c & 31), ...data];
  const mod = (polymod([...pre, 0, 0, 0, 0, 0, 0]) ^ 0x2bc830a3) >>> 0;
  const check = [0, 1, 2, 3, 4, 5].map((i) => (mod >>> (5 * (5 - i))) & 31);
  return `btx://${[...data, ...check].map((i) => A[i]).join("")}`;
}

describe("what may reach the clipboard", () => {
  it("accepts a 48-byte digest in lowercase hex and nothing else", () => {
    const id = "3d2b0dfa63231c4916dfc6d7f237393d1520c65b5d90008e07797a6aaf2824b0694536e184efa1f47bf14285b8bbdcbf";
    expect(id96(id)).toBe(id);
    for (const bad of [...HOSTILE, id.toUpperCase(), id.slice(1), `${id}0`, ` ${id}`, `${id}\n`, `${id.slice(0, 95)}g`, 12345, null, undefined, {}, [id]]) {
      expect(id96(bad)).toBe("");
    }
  });

  it("accepts a SHA-256 in lowercase hex and nothing else", () => {
    const h = "02b42f489691c7faa496c2e025aca3d2fa3d7a4197ebd72ee0d439bfa8c9a11c";
    expect(hash64(h)).toBe(h);
    for (const bad of [...HOSTILE, h.toUpperCase(), h.slice(1), `${h}0`, `${h}'`, "a".repeat(96), 5, null]) expect(hash64(bad)).toBe("");
  });

  it("accepts a block height and nothing that could be more than a number", () => {
    expect(blockHeight(400000)).toBe(400000);
    expect(blockHeight("400000")).toBe(400000);
    expect(blockHeight(1)).toBe(1);
    expect(blockHeight(499_999_999)).toBe(499_999_999);
    for (const bad of [0, -1, 500_000_000, 1.5, NaN, Infinity, "4e5", "400000; rm", "0x10", " 4", "", null, undefined, {}, "1".repeat(20)]) {
      expect(blockHeight(bad)).toBe(0);
    }
  });

  it("accepts the links the backend accepts: AInode's own four and a campaign's", () => {
    for (const l of [...CATALOG, CAMPAIGN_LINK]) expect(modelLink(l)).toBe(l);
    // The engine gives a bounty's record a link of the model kind too (measured:
    // v16/x-3-after-bounties.json). So the link alone does not tell a bounty
    // from a model; the view keeps bounties out by their kind (netview isBounty,
    // and the backend's search asks for object_kind MODEL).
    expect(modelLink(BOUNTY_LINK)).toBe(BOUNTY_LINK);
  });

  it("refuses what the backend refuses (validate::model_uri's own cases)", () => {
    const good = CATALOG[0];
    // One character changed: the checksum no longer matches.
    const flipped = `${good.slice(0, 20)}${good[20] === "q" ? "p" : "q"}${good.slice(21)}`;
    for (const bad of [
      flipped, good.toUpperCase(), good.slice(6), good.slice(0, -1), `${good}q`, "btx:///etc/passwd", "/Users/me/model.btx", "btx://pq1notours", "",
      // A link of another kind than a model (kind 1 here): the checksum is valid, the kind is not.
      otherKind(good),
      // The copied form has no space and no trailing slash, and nothing after it.
      ` ${good}`, `${good} `, `${good}/`, `${good}\n`, `${good};touch /tmp/x`, `${good}$(id)`, `${good}'`,
      ...HOSTILE, null, undefined, 7, { uri: good },
    ]) {
      expect(modelLink(bad)).toBe("");
    }
  });
});
