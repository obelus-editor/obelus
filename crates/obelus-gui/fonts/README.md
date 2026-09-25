# The glyphs Obelus draws, carried rather than hoped for

`SymbolsNerdFontMono-Regular.ttf` is Symbols Nerd Font Mono 3.5.1, from
[nerd-fonts](https://github.com/ryanoasis/nerd-fonts) release
`NerdFontsSymbolsOnly.zip`, unmodified. It is compiled into `obg` with
`include_bytes!` and registered as the last font in the fallback chain, so a
reader who has installed nothing still sees the marks -- which is one of the
two reasons the window exists at all.

Only the symbols. Letters, digits and CJK come from the fonts this machine
has: a font covering those is ten times this one's size, and which face
prose is set in is the reader's to choose.

## What it is under

The font itself is MIT, and `LICENSE` beside it is that notice, which has to
travel with any copy -- including the copy inside the binary.

The glyphs in it come from fourteen icon sets, each under its own licence.
All of them permit redistribution; the ones that are not MIT ask for
attribution, which is what this file is.

| Icon set | Upstream | Version | Licence |
|---|---|---|---|
| Codicons | https://github.com/microsoft/vscode-codicons | 0.0.45 | CC BY 4.0 |
| Devicons | https://github.com/devicons/devicon | 2.17.0 | MIT |
| extraglyphs | https://github.com/source-foundry/Hack | - | MIT |
| Font Awesome | https://github.com/FortAwesome/Font-Awesome | 6.5.1 | CC BY 4.0 |
| Font Awesome Extension | https://github.com/AndreLZGava/font-awesome-extension | 0.0.3 | MIT |
| Font Logos | https://github.com/lukas-w/font-logos | 1.3.0 | unlicensed |
| MaterialDesign | https://github.com/Templarian/MaterialDesign-Font | Oct 6, 2022 | Apache 2.0 |
| Octicons | https://github.com/primer/octicons | 18.3.0 | MIT |
| Seti and original | https://github.com/jesseweed/seti-ui | 0.8.1 | MIT |
| Pomicons | https://github.com/gabrielelana/pomicons | 1.001 | OFL 1.1 RFN |
| Powerline Extra | https://github.com/ryanoasis/powerline-extra-symbols | 1.200 | MIT |
| Powerline Symbols | https://github.com/powerline/powerline | 1.000 (ca 2013) | MIT |
| Power Symbols IEC | https://github.com/jloughry/Unicode | Feb 2015 | MIT |
| Weather Icons | https://github.com/erikflowers/weather-icons | 2.0.10 (1.100) | OFL 1.1 |

Two of those have a condition worth naming, because both are conditions on
*changing* the font rather than on carrying it:

* Pomicons is OFL with a reserved font name. Carrying this file under its own
  name is exactly what the licence is for; a subset or a rebuild may not be
  called Symbols Nerd Font.
* Font Logos is the distributions' own marks. A logo drawn to stand for the
  thing it is the logo of is what a trademark is for, and Obelus draws them
  nowhere else.
