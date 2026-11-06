# Security policy

## Supported versions

Only the latest release receives security fixes.

## Reporting a vulnerability

Please do not open a public issue. Report it privately through GitHub: open the
[Security tab](https://github.com/AdrienGras/vpn-switcher/security) of the
repository and click "Report a vulnerability".

Include the version (`vpns --version`), your OS, and the steps to reproduce.
You can expect an answer within a few days; a fix is released as a new version
and credited in the changelog unless you prefer otherwise.

## Scope

`vpns` runs the shell commands written in your configuration file, with your
privileges, through `sh -c`. Anyone who can edit that file can run commands as
you: keep it writable by you only. A report about running the commands you
configured is not a vulnerability; one about `vpns` running something you did
not configure (for example through a crafted profile name picked up by
`vpns init`) is.
