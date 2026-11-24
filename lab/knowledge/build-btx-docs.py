"""Build AInode's BTX library: upstream's own documentation, split into passages the chat
can hand a model when the conversation is about BTX.

  python3 lab/knowledge/build-btx-docs.py ~/repos/btx app/src/knowledge [ref]

Reads files with `git show <commit>:<path>` at `ref` (default HEAD; a tag such as
v0.34.16 reads that release without checking it out: the checkout is never touched) and writes
btx-docs.json: [{t: "<doc> > <section>", x: passage, src: path}], plus the commit it
came from. Upstream is MIT. Curated, not everything: internal trackers and proposals
would let a small model present plans as shipped facts.
"""
import json, pathlib, re, subprocess, sys

REPO, OUT = pathlib.Path(sys.argv[1]).expanduser(), pathlib.Path(sys.argv[2])
REF = sys.argv[3] if len(sys.argv) > 3 else "HEAD"
git = lambda *a: subprocess.run(["git", "-C", str(REPO), *a], check=True, capture_output=True, text=True).stdout
COMMIT = git("rev-parse", f"{REF}^{{commit}}").strip()
TAG = git("describe", "--tags", COMMIT).strip()
# The files of that commit, not of the checkout's index.
ls = lambda d: git("ls-tree", "-r", "--name-only", COMMIT, "--", d).split()

FILES = ["README.md", "HUMANS.md",
         "doc/design/btx-decentralized-frontier-ai-lab.md", "doc/design/0.34-ai-native-bitcoin.md",
         "doc/design/0.34-release-phases.md", "doc/btx-download-and-go.md", "doc/pay-with-compute.md",
         "doc/btx-miner-pool-readiness.md", "doc/btx-matmul-benchmarks.md"]
# Release notes: in doc/release-notes/ up to 0.34.15; 0.34.16's is doc/release-notes-0.34.16.md.
FILES += sorted(f for f in ls("doc") if re.fullmatch(r"doc/(release-notes/)?release-notes-0\.34\.\d+\.md", f))
# The model network's own docs, top level only: crf/, crl12/ and hcp/ are acceptance
# catalogues ("Given... When... Then..."), and these are machine tables or internal audits.
SKIP = {"rpc.md", "agent-recipes.md", "libp2p-ipfs-parts-bin-audit.md", "libtorrent-parts-bin-audit.md",
        "cuda-not-run.md", "connectivity-test-lab.md", "htlc-reuse.md", "events.md", "examples.md"}
FILES += sorted(f for f in ls("doc/modelnet")
                if f.endswith(".md") and f.count("/") == 2 and f.rsplit("/", 1)[1] not in SKIP)
MAX = 800
# Plain words for the sections that answer questions people ask in plain words: no doc
# says "the plan", but these sections are it (titles weigh three times in search.ts).
ALIASES = {
    ("HUMANS.md", "What 0.34.7 is"): "the plan, vision and goal of BTX",
    ("HUMANS.md", "Two markets"): "how models get funded: release campaigns and creation bounties",
    ("HUMANS.md", "Who participates"): "who takes part and how",
    ("doc/design/btx-decentralized-frontier-ai-lab.md", "Cyber Currency, Cognitive Capital, and the Rise of the Agentic State"):
        "the long-term plan and vision",
    ("doc/release-notes-0.34.16.md", "Upgrading from 0.34.15"):
        "what changed in 0.34.16",
}


def atx(md):
    """Underlined (setext) headings as # headings: 0.34.16's notes use them."""
    md = re.sub(r"^(\S[^\n]*)\n=+[ \t]*$", r"# \1", md, flags=re.M)
    return re.sub(r"^(\S[^\n]*)\n-{3,}[ \t]*$", r"## \1", md, flags=re.M)


def title_of(path, text):
    m = re.search(r"^#\s+(.+)$", text, re.M)
    t = (m.group(1) if m else pathlib.Path(path).stem).strip()
    # 0.34.16's notes are titled with the bare version: name them like the others.
    return f"BTX {t} release notes" if re.fullmatch(r"0\.34\.\d+", t) else t


def clean(md):
    md = re.sub(r"```.*?```", lambda m: m.group(0) if len(m.group(0)) <= 300 else "", md, flags=re.S)
    md = re.sub(r"<!--.*?-->", "", md, flags=re.S)
    md = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", md)                 # images
    md = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", md)             # links -> their text
    md = re.sub(r"[*_]{2}([^*_]+)[*_]{2}", r"\1", md)            # bold
    return md


def pieces(text):
    """Paragraphs packed into passages of at most MAX characters."""
    out, cur = [], ""
    for para in re.split(r"\n\s*\n", text):
        para = re.sub(r"\s+", " ", para).strip()
        if not para:
            continue
        while len(para) > MAX:  # one long paragraph: cut at a sentence
            cut = para.rfind(". ", 0, MAX)
            cut = cut + 1 if cut > MAX // 2 else MAX
            out.append(para[:cut].strip()); para = para[cut:].strip()
        if len(cur) + len(para) + 1 > MAX and cur:
            out.append(cur); cur = para
        else:
            cur = f"{cur} {para}".strip()
    if cur:
        out.append(cur)
    return out


docs = []
for path in FILES:
    raw = atx(git("show", f"{COMMIT}:{path}"))
    doc = title_of(path, raw)
    section = doc
    for block in re.split(r"^(#{1,4} .+)$", clean(raw), flags=re.M):
        if re.match(r"^#{1,4} ", block):
            section = block.lstrip("#").strip()
            if doc == f"BTX {section} release notes":  # the bare-version title itself
                section = doc
            continue
        for p in pieces(block):
            if len(p) >= 120 and p.count("|") < len(p) / 40:  # not a table
                t = doc if section == doc else f"{doc} > {section}"
                alias = ALIASES.get((path, section))
                docs.append({"t": f"{t} ({alias})" if alias else t, "x": p, "src": path})

# One passage for "what are the latest updates?": the release-note titles, newest first.
# The notes carry no dates; the date each was first committed stands in, and says so.
notes = sorted((f for f in FILES if "release-notes-0.34." in f), key=lambda f: int(re.search(r"0\.34\.(\d+)", f).group(1)), reverse=True)
lines = []
for f in notes:
    text = atx(git("show", f"{COMMIT}:{f}"))
    head = re.search(r"^#\s+(.+)$", text, re.M)
    head = head.group(1) if head else ""
    if re.fullmatch(r"0\.34\.\d+", head):
        # A bare version (0.34.16): its first sentence says what the release is.
        first = re.search(r"^#[ \t]+[^\n]+\n\s*(.+?\.)\s", text, re.M | re.S)
        # Without its full stop: the list is one sentence, and search.ts hands a model whole
        # sentences, so a stop here would cut the older releases off.
        what = re.sub(r"\s+", " ", first.group(1)).rstrip(".") if first else ""
        # and what its notes cover, from their section titles
        covers = [h for h in re.findall(r"^##\s+(.+)$", text, re.M) if h not in ("Included", "Not included")]
        if covers:
            plain = lambda h: h[0].lower() + h[1:] if h.split()[0][1:].islower() else h  # not "SwapNet"
            what += "; its notes cover " + ", ".join(plain(h) for h in covers)
        head = f"BTX {head} — {what}" if what else f"BTX {head}"
    if not head.startswith("BTX "):
        continue
    added = git("log", "--diff-filter=A", "--format=%cs", "-1", COMMIT, "--", f).strip()
    lines.append(f"{head.removeprefix('BTX ').replace(' — ', ': ')} (notes committed {added}).")
# One sentence per release: the chat hands a model whole sentences up to a limit
# (search.ts bestWindow, 700 characters for BTX notes), newest first, so the
# newest releases always fit. As one long sentence the list did not fit at all.
docs.append({"t": "BTX releases: the latest updates, newest first",
             "x": f"The shipping release is {TAG}. Newest first: " + " ".join(lines),
             "src": "doc/release-notes"})

(OUT / "btx-docs.json").write_text(json.dumps({"commit": COMMIT, "tag": TAG, "docs": docs},
                                               ensure_ascii=False, separators=(",", ":")) + "\n")
size = (OUT / "btx-docs.json").stat().st_size
print(f"{len(FILES)} files, {len(docs)} passages, {size / 1e6:.2f} MB, {TAG} {COMMIT[:8]}")
