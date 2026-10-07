# Contributing to lazytypst

This guide tells you how to change lazytypst and how to send the change.

## Before you start

1. Read the [Scope](README.md#scope) of the project. An idea that does not fit it is closed.
2. Pick an open issue, or open a new issue first.
3. Write a comment on the issue before you start. Then two people do not do the same work.
4. Make one pull request for one issue.

## Build and test

You need Rust 1.90 or later, and the `typst` command in `PATH`. Many tests run the real `typst` command. The tests run on Linux only, because some of them read `/proc`.

These four commands must pass before you send a change:

```
cargo build
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

## Code rules

1. `cargo fmt` formats the code. `cargo clippy` shows no warning.
2. Each change has tests. A bug fix starts with a test that fails before the fix.
3. Use the standard library when it does the work. A new dependency needs a reason in the pull request.
4. A change to a key or an option also changes the README and the `--help` text.
5. A change that a user can see adds one line under `Unreleased` in [`CHANGELOG.md`](CHANGELOG.md). Put the line in the section Added, Changed, Fixed, or Removed.

## Commits

1. Make one logical change in each commit. The build and the tests pass at every commit.
2. Write the subject in the imperative mood. Use 50 characters or fewer, and no final period. For example: `Show line numbers in the editor`.
3. Write in the body what changed and why.
4. Write `Closes #<number>` in the body to close the issue.
5. Keep a change to the format of the code in its own commit, with no other change.

## Pull requests

1. Describe what you tested and how.
2. Add a screenshot when the change is on screen.

## License

lazytypst uses the GNU General Public License, version 3 or any later version. Your contribution is under the same license.
