#!/usr/bin/env python3
"""Build emoji.tsv from Unicode's emoji-test.txt and CLDR annotations.

  python3 -I make-emoji.py emoji-test.txt en.json en-derived.json id.json id-derived.json > emoji.tsv

Inputs: https://www.unicode.org/Public/17.0.0/emoji/emoji-test.txt and, from
github.com/unicode-org/cldr-json (48.0.0), cldr-annotations-full/annotations/{en,id}/annotations.json
and cldr-annotations-derived-full/annotationsDerived/{en,id}/annotations.json.
One line per fully-qualified emoji without a skin-tone modifier (as in Omarchy's
own picker): emoji, English name, English keywords, Indonesian name,
Indonesian keywords; keyword lists are joined with "|".
"""
import json, sys

test, *cldr = sys.argv[1:]
ann = [{}, {}]
for i, path in enumerate(cldr):
    (top,) = json.load(open(path, encoding="utf-8")).values()
    for k, v in top["annotations"].items():
        ann[i // 2].setdefault(k, v)

def look(lang, e):
    a = ann[lang].get(e) or ann[lang].get(e.replace("️", "")) or {}
    return (a.get("tts") or [""])[0], a.get("default") or []

for line in open(test, encoding="utf-8"):
    if "; fully-qualified" not in line:
        continue
    cps = line.split(";")[0].split()
    if any(0x1F3FB <= int(c, 16) <= 0x1F3FF for c in cps):
        continue
    e = "".join(chr(int(c, 16)) for c in cps)
    name = line.split("#", 1)[1].split(None, 2)[2].strip()
    _, en = look(0, e)
    idn, idk = look(1, e)
    clean = lambda ws, n: "|".join(w for w in dict.fromkeys(ws) if w.lower() != n.lower())
    print("\t".join([e, name, clean(en, name), idn, clean(idk, idn)]))
