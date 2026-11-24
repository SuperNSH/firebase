# AGENTS.md

The brief for this repository, with its goal, sandbox, rules and satisfaction criteria, is [docs/PROMPT.md](docs/PROMPT.md). Read it first. Its hard rules bind every agent and subagent here. Track progress in `docs/STATUS.md`.

## Releasing

Bump the version in `Cargo.toml` (`[workspace.package]`) and `packages/*/package.json` together, add the CHANGELOG entry, push, then tag `vX.Y.Z`. The Publish crates workflow uploads to crates.io once CI has passed on the tagged commit. A crates.io version cannot be replaced, so push the tag only when the release is final. To retry: `gh workflow run publish.yml -f tag=vX.Y.Z`.
