# ADR 0004: What kasl may do in GitLab

- Status: accepted
- Date: 2026-11-02

## Context

kasl's protocol asks an inbox source for three things: list what is
assigned, move an issue (`start` or `close`), and comment on it. It was
written against Jira, whose workflows have categories - "in progress",
"done" - and resolutions. GitLab has neither: an issue is open or closed,
teams mark work in progress with a label of their own choosing, and a merge
request is finished by merging it.

The plugin also writes to a tracker the whole team reads, so a mistake there
is a mistake in front of everyone.

## Decision

- **Keys are GitLab's full references**, `group/app#14` for an issue and
  `group/app!31` for a merge request. They say where the thing lives, so a
  `transition` or a `comment` needs nothing kasl did not keep, and they link
  when pasted into a GitLab comment.
- **The inbox lists three kinds**: open issues assigned to the person, open
  merge requests assigned to them (type `Merge request`) and open merge
  requests they are asked to review (type `Review`). A merge request that is
  both is listed once, as assigned. A person who does not want reviews in
  the inbox dismisses them with an inbox rule on `type`; there is no setting
  for it.
- **Starting is carrying a label**: the one named with `--to`, or the
  `start_label` setting. The label must already exist in the project or its
  groups - GitLab would create a label it does not know on the spot, so a
  typo would leave a new label in the project. Without a label the plugin
  refuses and says where to name one; it never guesses. Work that carries
  the label shows it as its status.
- **Closing is for issues only.** A merge request is refused: merging it is
  not kasl's to do, and closing it unmerged throws the work away. A
  resolution or a target status is refused rather than dropped, because
  GitLab keeps neither.
- **Everything that can refuse is asked before the first write**: the item is
  open, the label exists. The comment that comes with a close follows the
  close, so a retry after a failed comment closes nothing twice.
- **A listing is whole or nothing.** kasl marks gone whatever a listing does
  not name, so a page that fails, or more pages than the plugin reads, fails
  the request instead of answering with a shorter list.

## Consequences

`kasl inbox start` does nothing until a label is named, which is one setup
step more than Jira needs - and the right one, since only the team knows
what its label is. Closing a merge request from kasl is impossible by
design. The status kasl shows for open work is `Open`, `Draft`, or the
start label; any other label a team uses for its workflow is not read.
