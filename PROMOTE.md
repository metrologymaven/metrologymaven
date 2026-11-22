# Promoting a library crate to its own repository

This procedure moves a crate out of the monorepo into its own repository without losing its commit history (`ECO.REPO.TPL.PROMOTE`). Automated steps never push to a remote; creating the remote and pushing are human steps.

## Roles

- **Proposer:** opens the promotion RFC, prepares the local extraction and consumer repin PR.
- **Maintainers (at least two, per governance):** approve the RFC, create the target remote repository, configure branch and tag protection, and push the extracted history.

## Steps

1. **Approve an RFC first (`ECO.COMM.RFC`).**
   Record why the crate is ready to leave the monorepo, its repository ID, target URL, and owning maintainers. Wait for the RFC comment period and sign-off before proceeding.
2. **Verify a clean monorepo checkout.**
   Check out the target commit on `main` with no uncommitted changes (`git status --porcelain` must be empty). `promote` refuses a dirty working tree (`PROMOTE-DIRTY`).
3. **Extract history and apply the repository template locally.**
   ```bash
   python3 -m repo_template promote \
     --source /path/to/monorepo \
     --path crates/<crate> \
     --id <repo-id> \
     --author "Maintainer Name <maintainer@example.org>" \
     --date "2026-11-22T00:00:00Z" \
     --out /path/to/<repo-id>
   ```
   The tool extracts every commit touching `crates/<crate>` with original authors, dates, and messages, verifies that the library's tree digest at the last extracted commit equals the source's, and applies the `code` repository template as one commit on top without modifying any library file. No git remote is added.
4. **Verify the extracted repository.**
   - Inspect `git -C /path/to/<repo-id> log --stat` and confirm the commit count equals extracted commits plus 1 template commit.
   - Confirm `git -C /path/to/<repo-id> remote` prints nothing.
   - Run `cargo test` inside `/path/to/<repo-id>`.
5. **Create the remote and push (human maintainer step).**
   - Create the new repository under the project organisation.
   - Enable branch protection on `main` and protect release tags (`v*`, matching `protected_tags = true` in `repos.toml`).
   - Add the remote manually and push `main`. Record the 40-hex commit hash `<rev>` of the pushed `HEAD`.
6. **Register the new repository.**
   - Add a `[[repo]]` entry to `repos.toml` with `id`, `url`, `purpose`, and `protected_tags = true`.
   - Add `<repo-id>` to the assigned blocks in `house-rules/manifest.toml` (and `manifest.json`).
7. **Repin consumers in the monorepo.**
   ```bash
   python3 -m repo_template repin \
     --workspace /path/to/monorepo \
     --crate <crate> \
     --url https://github.com/metrologymaven/<repo-id> \
     --rev <40-hex-rev>
   ```
   Remove `crates/<crate>` from the monorepo workspace members once consumers are repinned.
8. **Run the pin check and open the PR.**
   ```bash
   cargo xtask pins
   ```
   Ensure `pins: ok` and all workspace tests pass before opening the PR to `main`.
