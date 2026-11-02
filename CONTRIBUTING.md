# Contributing to kasl-plugin-gitlab

## The gate

```console
$ cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```

The same commands CI runs, so a green terminal means a green pull request.

`tests/protocol.rs` runs the built binary the way kasl runs it, against a
stand-in GitLab on a local port, and holds the answers to what the host accepts - a plugin that passes its unit tests and
fails there is one kasl would list and refuse.

## Layout

| Path | What lives there |
| --- | --- |
| `src/main.rs` | The manifest, and which request goes where |
| `src/protocol.rs` | The wire: an invocation in, a reply out, the settings |
| `src/gitlab.rs` | GitLab's REST API, as much as the plugin asks |
| `src/inbox.rs` | The inbox source: `list_issues`, `transition`, `comment` |
| `src/commits.rs` | `find_tasks`: the day's commits for `kasl task find` |
| `src/reference.rs` | Keys: GitLab's full references, `group/app#14` and `group/app!31` |
| `tests/protocol.rs` | The plugin run as the host runs it |
| `assets/` | The mark's three master SVGs |
| `docs/adr/` | Architecture decisions, newest last |

## Commits

[Conventional Commits](https://www.conventionalcommits.org/), in English:
`feat:`, `fix:`, `docs:`, `chore:` and the rest. The changelog is generated
from them (`cliff.toml`), so a commit message is the line the release notes
will carry.

## Releasing

A release is the end of a stage, and furca's release engine runs it from
[`release.toml`](release.toml): `furca release plan` says what the next
release is and what stands in its way, and `furca release run` carries it
out - bump, changelog, gate, commit, push, green CI, tag, publish - then
checks what went out. The notes of every release end with
[`.github/release-notes.md`](.github/release-notes.md), a checklist ticked on
the release page once the release has reached a machine the way a user gets
it and has been run by hand; `lyrn check --release` says which boxes are
still empty.

## Decisions

An architectural decision gets a record in [`docs/adr/`](docs/adr/README.md):
one file, numbered, newest last. A decision that replaces an earlier one says
so, and the earlier one is marked superseded rather than deleted.

## Dependencies

Updated by hand, at most once a week, in a commit of their own - Dependabot is
off on purpose ([ADR 0002](docs/adr/0002-dependabot-is-off.md)). The audit
workflow fails on a published advisory, and on a dependency whose license is
not on the accepted list.

## Assistant files

Instructions for coding assistants - `CLAUDE.md`, `AGENTS.md`, `.claude/`,
`.cursor/` and the like - stay on the machine they were written for, and
`.gitignore` keeps them out of the repository. What anyone needs to know about
the project, human or agent, is in this file and in [`llms.txt`](llms.txt).

## Conduct and security

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md). A
vulnerability is reported privately, as [SECURITY.md](SECURITY.md) describes -
never in a public issue.

## License

By contributing, you agree that your contributions are licensed under the
project's [MIT license](LICENSE).
