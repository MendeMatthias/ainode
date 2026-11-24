"""Fetch the opening paragraphs of Wikipedia's Vital Articles (Level 4), for an offline
general-knowledge pack. Text is CC BY-SA 4.0: the output keeps every title and the
revision fetched, for attribution.

  python3 lab/knowledge/fetch-vital.py <out-dir>            Level 4 (10,000 articles)
  python3 lab/knowledge/fetch-vital.py <out-dir> 5          Level 5 (about 45,000 more)

Polite by design: one request at a time, a pause between, maxlag, a User-Agent that
names the project (https://meta.wikimedia.org/wiki/User-Agent_policy).
"""
import json, pathlib, sys, time, urllib.parse, urllib.request

API = "https://en.wikipedia.org/w/api.php"
UA = "AInode-lab/0.2 (https://github.com/MendeMatthias/ainode; offline knowledge pack research)"
OUT = pathlib.Path(sys.argv[1]); OUT.mkdir(parents=True, exist_ok=True)


def get(**params):
    params.update(format="json", formatversion="2", maxlag="5")
    req = urllib.request.Request(f"{API}?{urllib.parse.urlencode(params)}", headers={"User-Agent": UA})
    for attempt in range(6):
        try:
            j = json.load(urllib.request.urlopen(req, timeout=60))
            if j.get("error", {}).get("code") == "maxlag":
                time.sleep(5 * (attempt + 1)); continue
            time.sleep(0.15)
            return j
        except Exception as e:  # network hiccup: back off and retry
            print("retry", e, file=sys.stderr); time.sleep(3 * (attempt + 1))
    raise SystemExit("gave up")


LEVEL = sys.argv[2] if len(sys.argv) > 2 else "4"
# Level 5 lists that are left out, on Mende's word that the bot should not care about
# politics (2026-10-12). Core history stays: it is in Level 4.
LEAVE_OUT = {"People/Politicians and leaders", "People/Military personnel, revolutionaries, and activists",
             "Technology/Weapons"}

if LEVEL == "4":
    # The eleven Level 4 topic lists. (Their old "Level/4/..." names are redirects
    # with no links of their own; alerts, archives and "Removed" are not lists.)
    TOPICS = ["Arts", "Biology and health sciences", "Everyday life", "Geography", "History", "Mathematics",
              "People", "Philosophy and religion", "Physical sciences", "Society and social sciences", "Technology"]
    pages = [f"Wikipedia:Vital articles/Level 4/{t}" for t in TOPICS]
else:
    # Level 5: every list page with content (the short ones only link to sub-lists).
    j = get(action="query", generator="allpages", gapnamespace=4, gapprefix=f"Vital articles/Level {LEVEL}/",
            gaplimit="max", gapfilterredir="nonredirects", prop="info")
    pages = sorted(p["title"] for p in j["query"]["pages"]
                   if p["length"] > 5000 and "Article alerts" not in p["title"]
                   and p["title"].split(f"Level {LEVEL}/", 1)[1] not in LEAVE_OUT)
    print(f"{len(pages)} list pages", flush=True)

# 2. The articles they link to (main namespace only).
titles = set()
for p in pages:
    cont = {}
    while True:
        j = get(action="query", prop="links", titles=p, redirects=1, plnamespace=0, pllimit="max", **cont)
        for page in j["query"]["pages"]:
            titles.update(l["title"] for l in page.get("links", []))
        if "continue" not in j: break
        cont = j["continue"]
    print(f"  {p.split('Level ', 1)[1]}: {len(titles)} so far", flush=True)
if LEVEL != "4" and (OUT / "vital4.json").exists():  # fetched already
    have = {d["t"] for d in json.loads((OUT / "vital4.json").read_text())}
    titles -= have
titles = sorted(titles)
print(f"{len(titles)} linked articles to fetch", flush=True)

# 3. Opening paragraphs, plain text, 20 per request, redirects followed.
docs = {}
for i in range(0, len(titles), 20):
    batch = titles[i:i + 20]
    j = get(action="query", prop="extracts|revisions", rvprop="ids", exintro=1, explaintext=1,
            exlimit=20, redirects=1, titles="|".join(batch))
    for page in j["query"].get("pages", []):
        text = (page.get("extract") or "").strip()
        if text and not page.get("missing"):
            docs[page["title"]] = {"t": page["title"], "x": text,
                                   "rev": (page.get("revisions") or [{}])[0].get("revid")}
    if i % 1000 == 0:
        print(f"  {i}/{len(titles)} -> {len(docs)} docs", flush=True)
(OUT / f"vital{LEVEL}.json").write_text(json.dumps(sorted(docs.values(), key=lambda d: d["t"]), ensure_ascii=False))
print(f"wrote {len(docs)} docs, {(OUT / f'vital{LEVEL}.json').stat().st_size / 1e6:.1f} MB", flush=True)
