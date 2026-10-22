"""Download OpenGameArt entries (files + page URL, author, licences) into <dir>/<slug>/, recording provenance.json.

Usage: python fetch.py <download dir> <slug> [<slug> ...]
"""
import sys, re, json, os, urllib.request, html, zipfile
D = sys.argv[1]
def get(u, b=False):
    r = urllib.request.urlopen(urllib.request.Request(u, headers={"User-Agent":"Mozilla/5.0"}), timeout=120).read()
    return r if b else r.decode("utf-8","replace")
prov = {}
pf = os.path.join(D, "provenance.json")
if os.path.exists(pf): prov = json.load(open(pf))
for slug in sys.argv[2:]:
    u = "https://opengameart.org/content/" + slug
    t = get(u)
    lics = re.findall(r"class='license-name'>(.*?)<", t)
    author = (re.findall(r'field-name-author-submitter.*?<a href="/users/[^"]+">([^<]+)<', t, re.S) or ["?"])[0]
    title = html.unescape(re.search(r'<title>(.*?) \|', t).group(1))
    files = [f for f in re.findall(r'href="(https://opengameart.org/sites/default/files/[^"]+)"', t) if not f.endswith(('.css','.png','.jpg'))]
    d = os.path.join(D, slug); os.makedirs(d, exist_ok=True)
    for f in files:
        name = urllib.parse.unquote(f.split('/')[-1])
        p = os.path.join(d, name)
        if not os.path.exists(p):
            open(p, "wb").write(get(f, True))
        if name.endswith(".zip"):
            zipfile.ZipFile(p).extractall(os.path.join(d, name + "_x"))
    prov[slug] = {"page": u, "title": title, "author": author, "licences": lics, "files": files}
    print(slug, author, lics, len(files))
json.dump(prov, open(pf, "w"), indent=1)
