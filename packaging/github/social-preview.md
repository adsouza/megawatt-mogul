# GitHub social preview

`social-preview.png` is the 1280×640 repository social preview, uploaded under the repository's
Settings → General → Social preview. It is not generated artwork: `social_preview.py` composes it
from the [package icon](../linux/icon.png) on a navy gradient sampled from the icon's tile, with the
title and the main menu's subtitle (`menu-subtitle` in `assets/locales/en-US/ui.ftl`).

Text is set in Atkinson Hyperlegible Next (SIL Open Font License). Content keeps at least 70 px
from every edge, inside GitHub's recommended 40 px border. Regenerate from the repository root:

```bash
python3 -I packaging/github/social_preview.py \
  ~/Library/Fonts/AtkinsonHyperlegibleNext-ExtraBold.otf \
  ~/Library/Fonts/AtkinsonHyperlegibleNext-Regular.otf \
  packaging/github/social-preview.png
```
