# {{FAMILY_NAME}}: name, logo and third-party usage guidelines

Status: draft guidelines (spec node `OUTR.BRAND.GUIDE.NAME`). This file writes the family name only as the token `{{FAMILY_NAME}}`; `fid-outreach guide-check --publish` substitutes the accepted name from `brand/name.toml`. Nothing in this document is legal advice. Statements about what the law permits are the project's understanding, marked [unverified], and are pending review by counsel (decisions D-001 and D-002).

## 1. Why these rules exist

{{FAMILY_NAME}} is open-source software. The code is free to use, copy and change under its licence. The name and the logo are different: they tell people who made something. These rules keep that signal honest, so that nobody is led to think the project made, checked or endorses something it did not.

The software licence covers the code. It does not grant rights in the name or the logo [unverified]. These guidelines are the project's statement of how it is happy for the name and logo to be used.

## 2. The names

- **Family name:** {{FAMILY_NAME}}. Always written in full, with the capitalisation shown here. Never shortened to one of its words, never abbreviated to initials, never joined into one word.
- **App names:** the family name followed by one function word:
  - {{FAMILY_NAME}} Inspect
  - {{FAMILY_NAME}} Probe
  - {{FAMILY_NAME}} Network
  - {{FAMILY_NAME}} Report
- **First mention:** in running text, use the full app name the first time (for example "{{FAMILY_NAME}} Report"). Later mentions in the same piece may use the full name again; they should not drop the family name.
- **Technical identifiers:** the command line tool, crate names and tool names use the short technical prefix `fid` (for example `fid-outreach`). These identifiers are not the brand and are written in code formatting.
- **Not a verb, not a plural.** Write "measure with {{FAMILY_NAME}} Inspect", not a verb made from the name.
- **Translations:** the names are not translated.

## 3. How anyone may refer to the project

You do not need permission to refer to the project by name to say something true about it. In many places this kind of plain, accurate reference to a product by its name is generally treated as permitted ("nominative use") [unverified]. In practice that means:

- Use only as much of the name as you need to identify the project or app: the word mark in plain text is usually enough.
- Say only what is true. "Exports reports that {{FAMILY_NAME}} Report can open" is fine if it is accurate.
- Do not suggest that the project sponsors, certifies or endorses you, your product or your service.
- Keep your own name and logo more prominent than ours on your own material.

Allowed without asking, as long as the points above hold:

- Articles, reviews, tutorials, talks, courses and books about the project.
- Saying that you use the project, teach it, contribute to it or offer services around it ("we offer training on {{FAMILY_NAME}} Inspect").
- Links to the project website or repository using the name as link text.
- Unmodified redistributions of an official release, under its licence, with the name it ships with.

## 4. Compatible products: "works with" and "built on"

If your product reads or writes the project's formats, plugs into its interfaces, or is a modified version of the software, describe the relationship in words that put your name first:

| Situation | Suggested wording |
|---|---|
| Your product exchanges data with an app | "Your Product works with {{FAMILY_NAME}} Report" |
| Your product is built from the project's code | "Your Product, built on {{FAMILY_NAME}}" |
| A plug-in or extension you wrote | "Your Plug-in for {{FAMILY_NAME}} Inspect" |
| A modified build of the software | Your own product name; mention "based on {{FAMILY_NAME}}" in the description |

Rules for these phrases:

- The claim must be true for the version you name, and you keep it true or remove it.
- Your own product name must not include the family name or an app name, and must not be confusingly close to them.
- A modified build must not be distributed under the family name or an app name, so users can tell an official release from a changed one.
- Do not use the phrases "official", "certified", "approved" or "partner" about your product's relationship with the project unless the project has said so in writing.

## 5. Uses that need written permission

These uses could suggest endorsement or official status, so they need written permission from the project maintainers first (open an issue in the project repository to ask):

1. Using the logo on a product, its packaging, its website header or its marketing material.
2. Any product, company, service, domain name, account name or event name that contains the family name or an app name.
3. Claiming certification, validation, partnership or official support from the project.
4. Merchandise (clothing, stickers, mugs and similar) with the name or logo, sold for profit.
5. Distributing a modified build under the family name or an app name.
6. Using the name or logo in a way that combines it with your own mark, so that the two read as one sign.

Permission, when given, is specific: it names who, what use and for how long, and can be withdrawn if the use changes.

## 6. The logo

The rules below are written relative to the logo's height `H` (the height of the symbol and word mark as supplied), so they hold for whatever final logo the asset pack (`OUTR.BRAND.ASSET.PACK`) contains.

### 6.1 Clear space

Keep an empty margin of at least `0.25 H` on every side of the logo. No text, image edge, other logo or busy background detail may enter this space. Where the logo sits in a frame or a button, the clear space is measured from the logo, not from the frame.

### 6.2 Minimum size

- On screens: `H` at least 24 CSS pixels for the logo with word mark, and at least 16 CSS pixels for the symbol alone.
- In print: `H` at least 8 mm for the logo with word mark, and at least 5 mm for the symbol alone.

Below these sizes, write the name in plain text instead of using the logo.

### 6.3 Forbidden alterations

Use the logo files exactly as supplied. In particular, do not:

- recolour it outside the palette variants supplied in the asset pack;
- stretch, squash, skew or rotate it, or change the proportions of its parts;
- add effects such as shadows, glows, outlines, bevels, gradients, textures or animation;
- crop it, rearrange its parts, or swap the typeface of the word mark;
- place it on a background that makes it hard to read; use the light or dark variant supplied for the background instead;
- combine it with another mark, name, badge or graphic into a single lock-up;
- use it as part of another logo, or as a pattern or decoration.

## 7. The project's own use

The project follows the same rules in its own material: the full family name, the app names as listed, the logo only as supplied, and no material that imitates another company's name, logo or trade dress. Other companies' products are referred to by name in plain text only when needed to state a fact (for example a file format the software can read), and never with their logos.

## 8. Questions and reporting misuse

Ask in the project repository's issue tracker. If you see a use that seems to suggest an endorsement the project did not give, report it there too. The maintainers will first ask politely for a correction.

## 9. Open points

- Trademark wording (registration marks, a licence of the mark) waits for counsel under D-001 and D-002 and is not drafted here.
- The minimum sizes in section 6.2 are proposals to be checked against the final logo files.
