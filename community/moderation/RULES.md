# Moderation rules per channel

These are the moderation rules for each official community channel. The behaviour expected of everyone is the code of conduct set by [GOVERNANCE.md](../../GOVERNANCE.md); it is not repeated here. How reports are handled is in [PROCEDURE.md](PROCEDURE.md). The website's channel list links each channel to its section below.

The channel IDs in the headings are the values used in the moderation log's `channel` field. The official channels themselves are chosen by spec node `OUTR.COMM.CHAN`; until that list is published, the sections below are the rules each kind of channel will get, and a channel added later gets its own section here before it opens.

## Rules for every channel

- Stay on topic for the channel's stated purpose.
- No advertising, spam or automated posting without the moderators' consent.
- Do not post anyone's personal data (addresses, phone numbers, private messages) without their consent.
- Do not paste copyrighted standards text; cite the designator and clause instead.
- Moderators act on reports and on what they see themselves, using the outcome categories in [PROCEDURE.md](PROCEDURE.md): no action, warning, removal, ban.

## `forum`: discussion forum

- **Report:** use the forum's report function on the post, or the private route.
- Moderators may move a thread to a better category or close a thread that has run its course; neither counts as a moderation outcome.
- Removed posts are hidden from public view; the moderation log does not keep their text.

## `chat`: real-time chat

- **Report:** mention the moderators' group in the channel or message a moderator listed in [roster.toml](roster.toml), or use the private route.
- Chat moves fast: moderators may remove a message first and explain afterwards.
- The public read-only archive shows the chat without removed messages.

## `issues`: issue tracker

- **Report:** use the host's report-content function, or the private route.
- Issues are for bugs and feature requests; support questions go to the forum.
- Moderators may lock a heated issue for a stated time; locking is not a moderation outcome unless it comes with a warning.

## `mailing-list`: announcement and discussion list

- **Report:** write to the list's moderator address, or use the private route.
- Posts from new members may be held for approval to stop spam.
- A removed post is deleted from the public archive; copies already delivered cannot be recalled.

## `private`: private report route

Not a discussion channel. It is the channel-independent route for reports (PROCEDURE.md section 1), and the channel ID used in the log for reports that came in this way.
