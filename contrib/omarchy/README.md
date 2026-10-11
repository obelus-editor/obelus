# Obelus under omarchy

[omarchy](https://omarchy.org) themes every program on the desktop at once:
one palette, a template per program, and a directory swapped into place. This
is Obelus's template for it, and what to do with it.

Nothing here is needed to use Obelus. What Obelus knows of omarchy is one
name and one place: `omarchy` is a theme it offers wherever omarchy has
written `~/.local/state/omarchy/current/theme/obelus.toml`, and this template
is what makes omarchy write it.

## What to do

Nothing, if Obelus came from `contrib/install.sh`: that script finds omarchy
-- by the two commands it uses -- and does the first step below itself, then
says the second; the template is a release asset, so it arrives checked
against the release's own sums like everything else that script downloads.
`--no-omarchy` is how to tell it not to. What follows is the same thing by
hand, for an Obelus built or packaged rather than installed.

Copy the template where omarchy looks for the ones you add yourself, and set
a theme once to render it the first time:

```bash
mkdir -p ~/.config/omarchy/themed
cp obelus.toml.tpl ~/.config/omarchy/themed/
omarchy-theme-set "$(omarchy-theme-current)"
```

Then choose `omarchy` in Obelus's list of themes, or say so in
`~/.config/obelus/config.toml`:

```toml
theme = "omarchy"
```

From then on, switching themes switches Obelus's colours -- including in an
Obelus that is already open, which needs no hook and no signal: Obelus
watches the directory the file is in.

An earlier install linked `~/.config/obelus/themes/omarchy.toml` at that file
instead. The install takes the link away now, because a theme of yours by
that name comes before the one Obelus finds by itself; made by hand, it is
yours to remove.

## How it works

`~/.config/omarchy/themed` is omarchy's own extension point, read beside its
built-in templates every time a theme is set. So this needs no change to
omarchy and no permission from anybody.

A theme in omarchy is a palette of semantic names -- `background`,
`foreground`, `red`, `muted`, `selection` -- and a template per program that
says which of them goes where. `obelus.toml.tpl` is that mapping for Obelus.
It is rendered into `~/.local/state/omarchy/current/theme/obelus.toml`, and
the whole of that directory is replaced atomically when a theme is set, which
is why Obelus reads it there rather than from a copy, and watches the
directory again each time it is replaced.

What the template leaves out is left out on purpose. An Obelus theme file
gives the colours it wants and inherits the rest from the built-in theme it
names -- `base = "{{ mode }}"`, which omarchy fills in with `dark` or
`light` -- so the colours omarchy has no word for stay Obelus's to choose.
It is also what keeps this file working when Obelus grows a new colour.

## Sending it upstream

If omarchy ever ships an Obelus template of its own it goes in
`default/themed/`, and that is the whole of the change: the renderer walks
that directory, and no hook is needed in `omarchy-theme-set` because Obelus
notices the file itself. Helix's `omarchy-restart-helix` sends `SIGUSR1`
because helix does not watch its own configuration.
