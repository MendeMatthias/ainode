"""Build AInode's quantum library from postquantum.wiki, Mende's post-quantum reference
(source: apps/wiki/content in the btx-apps monorepo).

  python3 lab/knowledge/build-pq-wiki.py ~/repos/btx-apps app/src/knowledge

Reads origin/main with `git show` (the working tree is never touched: it runs a live
service) and writes pq-wiki.json: [{t: "<article> > <section>", x: passage, src: url}].
Every article carries its own primary sources (CONTENT-SPEC.md); the passage keeps the
article's public URL so a model can point to it.
"""
import json, pathlib, re, subprocess, sys

REPO, OUT = pathlib.Path(sys.argv[1]).expanduser(), pathlib.Path(sys.argv[2])
REF = "origin/main"
git = lambda *a: subprocess.run(["git", "-C", str(REPO), *a], check=True, capture_output=True, text=True).stdout
COMMIT = git("rev-parse", REF).strip()
FILES = sorted(f for f in git("ls-tree", "-r", "--name-only", REF, "apps/wiki/content").split() if f.endswith(".md"))
MAX = 800


def front(md):
    m = re.match(r"^---\n(.*?)\n---\n", md, re.S)
    if not m:
        return {}, md
    meta = {}
    for key in ("title", "slug", "category", "description"):
        k = re.search(rf'^{key}:\s*"?(.*?)"?\s*$', m.group(1), re.M)
        if k:
            meta[key] = k.group(1)
    return meta, md[m.end():]


def clean(md):
    md = re.sub(r"```.*?```", lambda m: m.group(0) if len(m.group(0)) <= 300 else "", md, flags=re.S)
    md = re.sub(r"<!--.*?-->", "", md, flags=re.S)
    md = re.sub(r"!\[[^\]]*\]\([^)]*\)", "", md)
    md = re.sub(r"\[([^\]]+)\]\([^)]*\)", r"\1", md)
    md = re.sub(r"\[\[([^\]|]+)(?:\|([^\]]+))?\]\]", lambda m: m.group(2) or m.group(1).replace("-", " "), md)  # wiki links
    md = re.sub(r"[*_]{2}([^*_]+)[*_]{2}", r"\1", md)
    md = re.sub(r"\[\^?\d+\]", "", md)  # footnote marks
    return md


def pieces(text):
    out, cur = [], ""
    for para in re.split(r"\n\s*\n", text):
        para = re.sub(r"\s+", " ", para).strip()
        if not para:
            continue
        while len(para) > MAX:
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
    meta, body = front(git("show", f"{REF}:{path}"))
    title = meta.get("title") or pathlib.Path(path).stem
    url = f"https://postquantum.wiki/{meta.get('category', path.split('/')[-2])}/{meta.get('slug', pathlib.Path(path).stem)}"
    if meta.get("description"):
        docs.append({"t": title, "x": meta["description"], "src": url})
    section = title
    for block in re.split(r"^(#{1,4} .+)$", clean(body), flags=re.M):
        if re.match(r"^#{1,4} ", block):
            section = block.lstrip("#").strip()
            continue
        for p in pieces(block):
            if len(p) >= 120 and p.count("|") < len(p) / 40 and not section.lower().startswith(("sources", "references", "further reading")):
                docs.append({"t": title if section == title else f"{title} > {section}", "x": p, "src": url})

(OUT / "pq-wiki.json").write_text(json.dumps({"commit": COMMIT, "site": "postquantum.wiki", "docs": docs},
                                             ensure_ascii=False, separators=(",", ":")) + "\n")
size = (OUT / "pq-wiki.json").stat().st_size
print(f"{len(FILES)} articles, {len(docs)} passages, {size / 1e6:.2f} MB, {COMMIT[:8]}")
