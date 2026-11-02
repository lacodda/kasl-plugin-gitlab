<p align="center">
  <a href="https://github.com/lacodda/kasl-plugin-gitlab/actions"><img src="https://img.shields.io/github/actions/workflow/status/lacodda/kasl-plugin-gitlab/ci.yml?style=flat-square" alt="CI"></a>
  <a href="https://github.com/lacodda/kasl-plugin-gitlab/releases"><img src="https://img.shields.io/github/v/release/lacodda/kasl-plugin-gitlab?style=flat-square" alt="Release"></a>
  <a href="https://github.com/lacodda/kasl-plugin-gitlab/blob/main/LICENSE"><img src="https://img.shields.io/github/license/lacodda/kasl-plugin-gitlab?style=flat-square" alt="License"></a>
</p>

> The GitLab issues and merge requests waiting on you, in [kasl](https://github.com/lacodda/kasl)'s inbox - and the commits you pushed today, in `kasl task find`.

kasl is a work activity tracker for the command line. This plugin is its
GitLab: the issues assigned to you, the merge requests assigned to you and the
ones you are asked to review land in kasl's inbox beside Jira's, with toasts,
snoozes, triage and rules; `kasl inbox start`, `close` and `comment` reach back
into GitLab; and the commits you pushed on a day are offered as that day's
tasks.

## Install

It ships with kasl: the release archive, kasl's installers, its npm package
and `kasl self-update` put it beside the kasl binary. Set it up once:

```console
$ kasl plugin setup gitlab
```

| Setting | What to give it |
| --- | --- |
| `url` | The instance's root, such as `https://gitlab.com` |
| `token` | A personal access token - `api` to start, close and comment, `read_api` to read. Kept in the OS keyring. |
| `start_label` | Optional: the label your team puts on work in progress |

A kasl installed with `cargo install kasl-cli` has no plugin: take the archive
for your system from [Releases](https://github.com/lacodda/kasl-plugin-gitlab/releases)
and put the program in kasl's plugins directory
(`%LOCALAPPDATA%\lacodda\kasl\plugins` on Windows).

## In the inbox

```console
$ kasl inbox sync
✅ gitlab synced: 4 fetched, 4 new, 0 changed, 0 gone.
$ kasl inbox
+---+--------+-------+----------+--------------+--------+-----------------------------------+
| ★ | CHANGE | SCORE | PRIORITY | KEY          | STATUS | SUMMARY                           |
+---+--------+-------+----------+--------------+--------+-----------------------------------+
|   | NEW    | —     | —        | group/app#14 | Open   | Login fails on an expired session |
|   | NEW    | —     | —        | group/app#15 | Open   | Write the release notes           |
|   | NEW    | —     | —        | group/app!31 | Open   | Fix the expired session           |
|   | NEW    | —     | —        | group/lib!2  | Draft  | Bump the client                   |
+---+--------+-------+----------+--------------+--------+-----------------------------------+
$ kasl inbox start 'group/app#14' --to Doing
✅ group/app#14 is now Doing in gitlab.
$ kasl inbox close 'group/app!31'
❌ the plugin `gitlab` says: group/app!31 is a merge request: it is finished by merging it in GitLab, and kasl does not close merge requests
```

Keys are GitLab's own references. Starting puts a label on the work - the one
named with `--to`, or `start_label` - and only a label the project already has:
a typo is refused rather than created for the whole team. Closing is for issues;
a merge request is finished by merging it. The reasons are in
[ADR 0004](https://github.com/lacodda/kasl-plugin-gitlab/blob/main/docs/adr/0004-what-kasl-may-do-in-gitlab.md),
and the whole walk-through on
[kasl's site](https://kasl.lacodda.com/guides/gitlab-inbox/).

## The protocol

A plugin is an ordinary executable kasl runs for one call: `--manifest` says
what it offers, `run` reads one invocation on stdin and writes one reply on
stdout. This one answers four requests - `list_issues`, `transition` and
`comment` as an inbox source, `find_tasks` for `kasl task find` - as
[Writing a plugin](https://kasl.lacodda.com/guides/writing-a-plugin/) describes.

## Contributing

The gate, the layout and how commits are written are in
[CONTRIBUTING.md](https://github.com/lacodda/kasl-plugin-gitlab/blob/main/CONTRIBUTING.md).

## License

MIT - see [LICENSE](https://github.com/lacodda/kasl-plugin-gitlab/blob/main/LICENSE).
