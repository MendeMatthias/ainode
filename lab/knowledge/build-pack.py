"""Turn fetch-vital.py's output into the app's offline encyclopedia.

  python3 lab/knowledge/build-pack.py <vital4.json> [vital5.json ...] app/src/knowledge

Writes wiki.json (title + the whole opening paragraph, cut at a sentence near
2,000 characters) and wiki-attribution.json (every title and revision,
for CC BY-SA 4.0 attribution). Deterministic: same input, same bytes.
"""
import json, pathlib, re, sys

*srcs, out = [pathlib.Path(a) for a in sys.argv[1:]]
# Whole opening paragraphs, bounded: the chat shows each note as the subject's
# first sentence plus the sentences that best match the question (search.ts,
# notesFor), so facts deep in an intro still reach the model. 2,000 bounds the
# rare intro that runs for pages.
MAX = 2000


def clean(text):
    """Pronunciation and phonetics cost room and confuse small models: drop [IPA] and
    the empty "(; )" Wikipedia leaves when a pronunciation template is removed."""
    text = re.sub(r"\s*\[[^\]]{0,120}\]", "", text)
    text = re.sub(r"\b[A-Z][a-z]+(?: [A-Z][a-z]+)?:\s*;\s*", "", text)  # "Dutch: ;" left by the above
    text = re.sub(r"\(\s*[;,]\s*", "(", text)
    text = re.sub(r"\s*[;,]\s*\)", ")", text)
    text = re.sub(r"\(\s*\)", "", text)
    return re.sub(r"\s+", " ", text).strip()


def cut(text):
    text = clean(text)
    if len(text) <= MAX:
        return text
    head = text[:MAX]
    end = head.rfind(". ")
    return head[:end + 1] if end > MAX // 2 else head.rsplit(" ", 1)[0] + "…"


seen, docs = set(), []
for src in srcs:  # Level 4 first; a title already taken is not added again
    for d in json.loads(src.read_text()):
        if d["t"] not in seen:
            seen.add(d["t"]); docs.append(d)
docs.sort(key=lambda d: d["t"])
pack = [{"t": d["t"], "x": cut(d["x"])} for d in docs if len(d["x"]) >= 40]
(out / "wiki.json").write_text(json.dumps(pack, ensure_ascii=False, separators=(",", ":")) + "\n")
(out / "wiki-attribution.json").write_text(json.dumps({
    "source": "English Wikipedia, opening paragraphs of the Vital Articles (Levels 4 and 5, three political lists of Level 5 left out)",
    "licence": "CC BY-SA 4.0, https://creativecommons.org/licenses/by-sa/4.0/",
    "authors": "Wikipedia contributors; each article's history lists them: https://en.wikipedia.org/w/index.php?oldid=<rev>&action=history",
    "changes": "Opening paragraph only; pronunciation brackets removed; whitespace collapsed; cut at a sentence near 2,000 characters.",
    "articles": [[d["t"], d.get("rev")] for d in docs if len(d["x"]) >= 40],
}, ensure_ascii=False, separators=(",", ":")) + "\n")
raw = (out / "wiki.json").stat().st_size
print(f"{len(pack)} articles, wiki.json {raw / 1e6:.1f} MB")
