# AGENTS.md

The brief for this repository, with its goal, sandbox, rules and satisfaction criteria, is [docs/PROMPT.md](docs/PROMPT.md). Read it first. Its hard rules bind every agent and subagent here. Track progress in `docs/STATUS.md`.

## Releasing

Add a `## [X.Y.Z] - YYYY-MM-DD` entry at the top of `CHANGELOG.md` and push to `master`. The Changelog Release workflow bumps the versions, tags `vX.Y.Z`, creates the GitHub release with that entry as its notes and publishes to crates.io. Do not bump versions or push tags by hand. A crates.io version cannot be replaced, so CI must be green before the entry lands. To finish a release that stopped halfway: `gh workflow run changelog-release.yml`, or `gh workflow run publish.yml -f tag=vX.Y.Z` for an older tag.
