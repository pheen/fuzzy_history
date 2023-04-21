# **F**u**z**zy **H**istory

Fzh is a simple shell history search engine. Fuzzy find with ordering taking into account the current directory, exit status, datetime, and number of times ran for a command.

## Table of Contents

- [Installation](#installation)
- [Usage](#usage)
- [Commands](#commands)
- [Developer Commands](#developer-options)
- [Todo](#todo)

## Installation

To install Fzh, you can download the binary from the [Github Releases page](https://github.com/username/fzh/releases) and move it to `/usr/local/bin/`.

```
$ cd ~/Downloads
$ mv fzh /usr/local/bin/
```

## Usage

Populate the database with your current history by running an import:

```
$ fzh import zsh
```

This will index your Zsh command history and store it in `~/.fzh`. Only Zsh is currently supported.

Perform searches with the keybind `^R`.

## Commands

- `import <shell> [<path>]`: Index command history for a shell (path defaults to `~/.zsh_history`)
- `delete_index`: Remove all indexed command history

## Developer Commands

Fzh includes a few developer options that can be used to add commands to the index manually or start the search client manually:

- `search <text>`: Start a search client, the same as what's invoked from the keybind `^R`.
- `add <exit_code>:<text>`: Write a command to the index.

## Todo

- handle signals like cmd+backspace, cmd+left_arrow, etc.
