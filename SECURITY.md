# Security

The plugins here run in Kalem's sandbox with the permissions their
manifests declare, and a release is signed and listed in `index.json`
with its SHA-256. A plugin that reaches past its sandbox or its
permissions, a file that makes one run code or write where it should
not, or a flaw in how releases are built, signed or listed is a
security problem.

Please report one privately, through GitHub's *Report a vulnerability*
on this repository's Security tab, rather than in a public issue (a
problem in Kalem itself goes to getkalem/kalem's). Say what an attacker
gets and how to reproduce it; a fix is released as soon as it is ready,
and the report is credited unless you ask otherwise. Only each plugin's
latest release is supported.
