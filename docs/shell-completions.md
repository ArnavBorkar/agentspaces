# Shell completions

Generate completion scripts with:

```bash
asp completions bash
asp completions zsh
asp completions fish
asp completions powershell
asp completions elvish
```

For agent automation, use JSON output:

```bash
asp --json completions bash
```

## Install examples

Bash:

```bash
mkdir -p ~/.local/share/bash-completion/completions
asp completions bash > ~/.local/share/bash-completion/completions/asp
```

Zsh:

```bash
mkdir -p ~/.zfunc
asp completions zsh > ~/.zfunc/_asp
```

Then make sure `~/.zfunc` is in your `fpath`.

Fish:

```bash
mkdir -p ~/.config/fish/completions
asp completions fish > ~/.config/fish/completions/asp.fish
```
