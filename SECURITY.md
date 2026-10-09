# Security policy

## Report a problem in private

Do not open a public issue for a security problem. Use the private report form of this repository:

1. Open the Security tab: https://github.com/Orpheus-21/lazytypst/security
2. Choose "Report a vulnerability".
3. Write what you found.

Only the maintainers of the repository can read the report.

Write these facts in the report:

- The output of `lazytypst --version`.
- The steps that show the problem. One action in each step.
- What you expected, and what happened.
- Why it matters: what a person could do, and what they need for it.

## What happens next

The maintainer reads the report, and answers in the report. The answer says if the report is accepted. A fix goes into a new release, and the changelog names it. The maintainer asks you before the report is made public, and names you in the changelog if you want that.

## Which versions get fixes

Only the newest release gets fixes. Update first, then report.

## What is a security problem

These are in scope. Each is a way that a project, a file, or a terminal can harm the person who opens it:

- A file or a folder of a project writes, deletes, or reads a file outside the project, or outside the place that the program shows.
- Text of a file, a file name, or an error message runs a command, or changes the terminal in a way that the person did not ask for.
- A document takes all the time or all the memory of the machine, and the limits of the program do not stop it.
- A release archive does not match what GitHub built (see the build attestation in the README).

The section "Safety with projects from other people" of the README lists what the program does today.

## What is not in scope

- Problems in Typst itself. Report them to the Typst project: https://github.com/typst/typst
- Problems that need the person to run a command or to give the program a bad `LAZYTYPST_TYPST`, `VISUAL`, or `EDITOR` value. These variables are the person's own choice.
- A document that Typst reads from a link inside the project. The program warns about such links, and the README says that this is a behavior of Typst.
