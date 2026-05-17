# pinion-text-font test fixtures

Production-grade font binaries used by §5.37.1 sfnt parser tests.
Both files are redistributed under SIL Open Font License 1.1 — license texts in
`LICENSE-NotoSans.txt` and `LICENSE-NanumGothic.txt`.

| File | Source | License | Script coverage |
|---|---|---|---|
| `NotoSans-Regular.ttf` | github.com/notofonts/latin-greek-cyrillic | OFL-1.1 | Latin / Greek / Cyrillic |
| `NanumGothic-Regular.ttf` | github.com/google/fonts (ofl/nanumgothic) | OFL-1.1 | 한글 (KS X 1001 + Unicode CJK) |

R50.1.1 sfnt parser test 의 fixture. R50.1.2+ 의 head/OS2/cmap/glyf parser
도 같은 fixture 재사용.
