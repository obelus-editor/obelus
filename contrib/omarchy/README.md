# obelus under omarchy

[omarchy](https://omarchy.org) themes every program on the desktop at once:
one palette, a template per program, and a directory swapped into place. This
is obelus's template for it, and what to do with it.

Nothing here is needed to use obelus, and nothing in obelus knows about
omarchy. A theme is a file; this is a way of having that file written for
you.

## What to do

Copy the template where omarchy looks for the ones you add yourself, link
obelus's themes directory at what it renders, and say you want it:

```bash
mkdir -p ~/.config/omarchy/themed ~/.config/obelus/themes

cp obelus.toml.tpl ~/.config/omarchy/themed/

ln -sf ~/.local/state/omarchy/current/theme/obelus.toml \
       ~/.config/obelus/themes/omarchy.toml

# and in ~/.config/obelus/config.toml
theme = "omarchy"
```

Then set a theme once to render it the first time:

```bash
omarchy-theme-set "$(omarchy-theme-current)"
```

From then on, switching themes switches obelus's colours -- including in an
obelus that is already open, which needs no hook and no signal: obelus
watches the file, and the link tells it where the file really is.

## How it works

`~/.config/omarchy/themed` is omarchy's own extension point, read beside its
built-in templates every time a theme is set. So this needs no change to
omarchy and no permission from anybody.

A theme in omarchy is a palette of semantic names -- `background`,
`foreground`, `red`, `muted`, `selection` -- and a template per program that
says which of them goes where. `obelus.toml.tpl` is that mapping for obelus.
It is rendered into `~/.local/state/omarchy/current/theme/obelus.toml`, and
the whole of that directory is replaced atomically when a theme is set, which
is why the link points *into* it rather than at a copy.

What the template leaves out is left out on purpose. An obelus theme file
gives the colours it wants and inherits the rest from the built-in theme it
names -- `base = "{{ mode }}"`, which omarchy fills in with `dark` or
`light` -- so the colours omarchy has no word for stay obelus's to choose.
It is also what keeps this file working when obelus grows a new colour.

## Sending it upstream

If omarchy ever ships an obelus template of its own it goes in
`default/themed/`, and that is the whole of the change: the renderer walks
that directory, and no hook is needed in `omarchy-theme-set` because obelus
notices the file itself. Helix's `omarchy-restart-helix` sends `SIGUSR1`
because helix does not watch its own configuration.
