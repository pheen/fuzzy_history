# **F**u**z**zy **H**istory

Fzh is a simple shell history search engine that replaces `ctrl-r`. Fuzzy find with ordering taking into account the current directory, exit status, datetime, and number of times ran for a command.

Regular shell history is not affected as the search index is kept entirely separate.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [Commands](#commands)
- [Developer Commands](#developer-options)
- [Remaining Work](#remaining-work)

## Installation

1. To install Fzh, you can download the binary from the [Github Releases page](https://github.com/username/fzh/releases) and move it to `/usr/local/bin/`.

```
$ cd ~/Downloads
$ mv fzh /usr/local/bin/
```

2. Add the initializer to `~/.zshrc`.

```
# Setup Fzh keybinds and event hooks. Removing this will
# restore previous `ctrl+r` behaviour.
eval "$(fzh init zsh)"
```

3. Restart your terminal or run `. ~/.zshrc`.

## Usage

Populate the database with your current history by running an import:

```
$ fzh import zsh
```

This will index your Zsh command history and store it in `~/.fzh`. Only Zsh is currently supported.

Search with the keybind `ctrl-r` (`^R`).

## Commands

- `import <shell> [<path>]` Index command history for a shell (path defaults to `~/.zsh_history`)
- `init <shell>` Prints the init script (source with `eval \"$(fzh init zsh)\"`)
- `delete_index` Remove all indexed command history

## Developer Commands

Fzh includes a few developer options that can be used to add commands to the index manually or start the search client manually:

- `search <text>` Start a search client, the same as what's invoked from the keybind `^R`.
- `add <exit_code>:<text>` Write a command to the index.

## Remaining Work

- [ ] Handle signals like cmd+backspace, cmd+left_arrow, etc.
- [ ] Eat cake!
