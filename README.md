# Gitlab

> Brings the GitLab issues and merge requests assigned to you into kasl's inbox, and today's commits into kasl task find

A plugin for [kasl](https://github.com/lacodda/kasl), a work activity tracker for the command line.
Scaffolded with [lyrn](https://github.com/lacodda/lyrn).

## Install

Download the binary for your platform from [Releases](https://github.com/lacodda/kasl-plugin-gitlab/releases)
and put it where kasl looks: the `plugins` directory in its data folder (`%LOCALAPPDATA%\lacodda\kasl\plugins` on Windows), then your `PATH`.

kasl finds anything named `kasl-plugin-*` there and shows
**Run Gitlab** once this one is installed.

## The protocol

A plugin is an ordinary executable. kasl runs it twice:

```console
$ kasl-plugin-gitlab --manifest   # what it offers, as JSON on stdout
$ kasl-plugin-gitlab run          # invocation as JSON on stdin, outcome on stdout
```

The manifest declares the protocol version, a name, and the commands this
plugin contributes:

```jsonc
{
  "protocol_version": 1,
  "name": "gitlab",
  "version": "0.1.0",
  "commands": [
    { "key": "run", "label": "Run Gitlab", "target": "day" }
  ]
}
```

An invocation arrives as `{"command": "run", "target": "day", "subject": {…}}`,
where `subject` is the day as `kasl report --json` prints it, or the issue as `kasl inbox show --json` prints it. The reply may carry `meta` (fields
shown under the plugin's message - the host keeps no field a plugin writes), `message` (shown to the user), or `error`.

Plugins live for the duration of one call. Nothing here starts a service.

## Contributing

The gate, the layout and how commits are written are in
[CONTRIBUTING.md](https://github.com/lacodda/kasl-plugin-gitlab/blob/main/CONTRIBUTING.md).

## License

MIT - see [LICENSE](https://github.com/lacodda/kasl-plugin-gitlab/blob/main/LICENSE).
