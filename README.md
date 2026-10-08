# gray-prompt

Layered user prompt customization: a global file plus a per-project file,
both injected into every turn. Port of pi's `prompt-customizer` extension.

A sidecar plugin for [gray](https://github.com/vstaln/gray), scaffolded by
[gray-account](https://github.com/vstaln/gray-account).

## What it does

On `prompt/context` the sidecar injects every customization file that
exists, in order:

1. `~/.gray/prompt/custom.md` — global, applies to every session
   (`$GRAY_HOME` honored, `$HOME/.gray` fallback)
2. `<session.cwd>/.gray-prompt.md` — per-project

Each file is capped at 8 KiB. Nothing found → `{}` (nothing injected).

`/prompt` lists what the last `prompt/context` call actually injected
(paths + byte sizes).

## Wire methods

`plugin/manifest` · `prompt/context` (hook) · `command/run` (`/prompt`) ·
`plugin/shutdown`

## Install

```sh
gray plugin install prompt
```

## Develop

```sh
cargo test
gray account check      # entry point + manifest handshake
gray account publish    # check → build → release → publish to the gray registry
```

Bump `version` in `Cargo.toml` before each `publish`; the registry refuses to
republish a version.
