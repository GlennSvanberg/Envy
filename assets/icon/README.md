# Envy logo

The tile is green ("envy green") with a white E made of three lines, like the KEY=value rows in a .env file, plus a light text caret for keyboard editing.

- envy-icon.svg: master icon, used from 48 px and up
- envy-icon-small.svg: thicker version with no caret, used at 16–32 px
- envy-icon-{16,20,24,32,48,64,128,256,512}.png: rendered sizes
- envy.ico: Windows icon with every size from 16 to 256 (16–32 come from the small variant)
- envy-readme-header-{light,dark}.svg/.png: 1280×320 header with the wordmark and tagline (the PNG is safest on GitHub because of fonts)

Embedding in an exe (Rust): add envy.ico through winres/embed-resource in build.rs. Use envy-icon-256.png or envy-icon-64.png for the GPUI window icon.
