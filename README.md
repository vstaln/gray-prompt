<p align="center">
  <img src="assets/gray-logo.svg" alt="gray" width="96">
</p>
<h1 align="center">gray-prompt</h1>
<p align="center">Layered global and per-project prompt customization.</p>
<p align="center">
  <a href="https://github.com/vstaln/gray-prompt/blob/main/LICENSE"><img alt="MIT License" src="https://img.shields.io/badge/license-MIT-blue.svg"></a>
  <img alt="gray plugin" src="https://img.shields.io/badge/gray-plugin-7aa2f7.svg">
  <img alt="rust" src="https://img.shields.io/badge/built%20with-rust-orange.svg">
</p>

Inject user-managed prompt customizations into every turn: one global file
plus one per-project file.

## What it does

On `prompt/context`, the sidecar injects every customization file that
exists, in order:

1. `~/.gray/prompt/custom.md` — global, applies to every session
   (`$GRAY_HOME` honored, `$HOME/.gray` fallback)
2. `<session.cwd>/.gray-prompt.md` — per-project

Each file is capped at 8 KiB. When nothing is found, the hook returns `{}`
and injects nothing.

`/prompt` lists what the last `prompt/context` call actually injected:
source paths and byte sizes.

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

---
Part of the [gray](https://github.com/vstaln/gray) plugin ecosystem —
the open-source AI agent harness. <https://gray.alignment.id>
