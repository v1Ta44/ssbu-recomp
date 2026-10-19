# Regenerates ui/fonts/*.woff2: subsets of Smiley Sans (CJK) and Saira italic (Latin) holding only the characters the UI uses.
# Run after changing UI text:  python -I tools/console/subset_fonts.py tools/console/ui <folder with SmileySans-Oblique.ttf and saira-italic.ttf>
# (fonttools + brotli; the source fonts are the OFL releases from github.com/atelier-anchor/smiley-sans and google/fonts ofl/saira)
import sys, re, pathlib
from fontTools import subset
from fontTools.ttLib import TTFont
src = pathlib.Path(sys.argv[1])
text = ''.join(p.read_text(encoding='utf-8') for p in [src / 'index.html', src / 'app.js', src / 'style.css', *(src.parent / 'src').glob('*.rs')])
cjk = ''.join(sorted({c for c in text if ord(c) > 0x2000}))
ascii_ = ''.join(chr(i) for i in range(0x20, 0x7f))
extra = '×–—…·←↑→↓−＋：，。、（）“”'
def run(inp, out, chars):
    opts = subset.Options(); opts.flavor = 'woff2'; opts.layout_features = ['*']; opts.name_IDs = ['*']; opts.notdef_outline = True
    f = TTFont(inp); s = subset.Subsetter(opts); s.populate(text=chars); s.subset(f); f.flavor = 'woff2'; f.save(out)
d = pathlib.Path(sys.argv[2])
run(d / 'SmileySans-Oblique.ttf', src / 'fonts' / 'smiley.woff2', cjk + ascii_ + extra)
run(d / 'saira-italic.ttf', src / 'fonts' / 'saira.woff2', ascii_ + '×–—…·←↑→↓−')
# Backend tags and user-supplied mod names need glyphs beyond the static UI subset.
full = TTFont(d / 'SmileySans-Oblique.ttf'); full.flavor = 'woff2'; full.save(src / 'fonts' / 'smiley-full.woff2')
print(len(cjk), 'cjk chars')
# OFL Reserved Font Names: a subset is a modified version, so it carries other names
for fn, fam in (('smiley.woff2', 'SSBU Console CJK'), ('smiley-full.woff2', 'SSBU Console CJK Complete'), ('saira.woff2', 'SSBU Console Latin')):
    p = src / 'fonts' / fn; f = TTFont(p); nm = f['name']
    for rec in list(nm.names):
        if rec.nameID in (1, 3, 4, 6, 16, 17, 18, 21, 22, 25): nm.removeNames(nameID=rec.nameID)
    for i, v in ((1, fam), (2, 'Regular'), (3, fam + ' subset'), (4, fam), (6, fam.replace(' ', '') + '-Regular')):
        nm.setName(v, i, 3, 1, 0x409)
    f.save(p)
print('renamed')
