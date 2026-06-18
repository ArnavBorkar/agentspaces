# Manpage generation

Generate a roff manpage for the current `asp` binary:

```bash
asp manpage > asp.1
```

For automation, use JSON output:

```bash
asp --json manpage
```

The command is generated from the same clap command tree as `asp --help`, so it
stays aligned with CLI flags and subcommands.

Package maintainers can install it into the usual man directory for their
package format, for example:

```bash
install -Dm0644 asp.1 "$pkgdir/usr/share/man/man1/asp.1"
```
