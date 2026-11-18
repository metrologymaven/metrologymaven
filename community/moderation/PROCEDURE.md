# Moderation procedure

This page says how the project enforces its code of conduct on official community channels. It implements spec node `OUTR.COMM.MOD`. The code of conduct itself and the roles named here (maintainers, steering group) are set by [GOVERNANCE.md](../../GOVERNANCE.md); this page does not restate them. Per-channel rules are in [RULES.md](RULES.md); the people who moderate are listed in [roster.toml](roster.toml).

Figures marked *proposed* (the 72-hour acknowledgement target and the 24-month retention period) are proposals that the maintainers still have to confirm.

## 1. Reporting

Anyone may report behaviour on an official channel that they think breaks the code of conduct. There are two kinds of route:

- **A route on each channel.** Every official channel has its own way to reach the moderators, named in [RULES.md](RULES.md) next to that channel (for example a report button or a moderators' group to mention). The channel list on the website links each channel to its rules.
- **One private route that does not depend on any channel.** It reaches the moderators directly and is for reports that should not pass through the channel where the problem happened, or for people without an account there. Its address is published with the channel list on the website.

A report needs only what the reporter chooses to share. Nobody has to give their real name.

## 2. Acknowledgement

A moderator acknowledges every report that comes in through an official route within **72 hours** (*proposed*). The acknowledgement tells the reporter that a person has the report; it does not have to contain a decision. Each quarter the project publishes the share of reports acknowledged within the target (section 7).

## 3. Outcomes

Every report ends in exactly one of four outcome categories:

| Category | Log value | Meaning |
|---|---|---|
| No action | `no_action` | The report was looked at and nothing on the channel needs to change. |
| Warning | `warning` | The person was told which rule they broke; nothing was removed. |
| Removal | `removal` | One or more posts were taken down (with or without a warning). |
| Ban | `ban` | The person may no longer take part on one or more official channels, for a stated time or permanently. |

The moderator tells the reporter which category the report ended in, but not the details of any action against another person.

## 4. Conflicts of interest

A moderator has a conflict of interest in a case when they are the reporter or the reported person, are a close colleague, friend or relative of either, or for any other reason could not be seen as fair. A moderator with a conflict:

1. says so as soon as they notice it;
2. hands the case to another moderator without a conflict, and takes no further part in it;
3. is recorded in the case's log entry under `parties_conflicted`, and the hand-over under `handover` (from whom, to whom).

A case is never decided by a moderator listed as conflicted in it. If every moderator has a conflict, the case goes to the steering group (section 5).

## 5. Escalation to the steering group

A case goes to the steering group when:

- the reported person is a moderator, a maintainer or a member of the steering group;
- every moderator has a conflict;
- the outcome would be a permanent ban; or
- the reporter or the reported person asks for a review of a decision.

Steering-group members with a conflict follow section 4 in the same way. The steering group's decision is final and is recorded in the same log.

## 6. The moderation log

Moderators record each case by hand in a private moderation log. No tool writes it.

- **Where.** The log is kept **outside every public repository**, in a place every steering-group member can read and only moderators and the steering group can change. The project's tools refuse to read a log that lies inside a repository checkout. Which storage is used is a maintainer choice; it must not hand the log's contents to third parties.
- **What.** Each entry holds only: a case ID, the date and time filed and acknowledged (UTC), the channel ID, the outcome category, the handling moderator's handle, the handles of moderators with a conflict, and the hand-over if one happened. It holds **no** names, e-mail addresses or other contact details, and **no** text, screenshots or links of the posts concerned. Working notes that a case needs while it is open are kept by the moderators elsewhere and deleted when the case is closed.
- **Who reads it.** Every steering-group member may audit the log at any time.
- **Retention.** After **24 months** (*proposed*) from the date filed, an entry is anonymised: it is replaced by an entry that keeps only the quarter, the outcome category and whether the acknowledgement target was met.

The log format is a TOML file of `[[case]]` and `[[anon]]` entries, documented in the `fid-outreach` crate (module `moderation`). Two read-only commands work on it:

- `fid-outreach mod-check --log <file> --roster community/moderation/roster.toml --today <YYYY-MM-DD>` checks every entry: required fields, outcome categories, unknown moderators, conflicts that were not handed over, and entries past the retention period. It names cases by ID only.
- `fid-outreach mod-stats --log <file> --quarter <YYYYQn> --target-hours 72 --out <file>` writes one quarter's counts per outcome category, the total, and how many reports were acknowledged within the target.

Run `mod-check` before every quarterly report and anonymise any entry it reports as `retention_overdue`.

## 7. What is published

Once a quarter, the counts from `mod-stats` appear in the public adoption report: the number of reports per outcome category, the total, and the share acknowledged within the target. Nothing else from the log is published: no case IDs, dates, channels or handles.

## 8. Changing this procedure

Changes to this page follow the decision process in [GOVERNANCE.md](../../GOVERNANCE.md).
