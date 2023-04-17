#!/bin/zsh

if [[ -o interactive ]]; then # Check stdin is a tty

  ##  Setup  ##################################################################

  # Find the binary
  FZH_PATH=${FZH_PATH:-$(command which fzh)}
  if [[ -z "$FZH_PATH" || "$FZH_PATH" == "fzh not found" ]]; then
    echo "`fzh` binary is missing. Please add it to your path before sourcing fzh.zsh"
    return 1
  fi

  ##  Bind hooks  #############################################################

  # First register with `zshaddhistory` to access and store the last command.
  # This is called before the command is actually executed so the exit status is
  # not available.
  fzh_add_history_hook() {
    export FZH_LAST_CMD="$1"
  }
  add-zsh-hook zshaddhistory fzh_add_history_hook

  # `precmd_functions` are ran just before the prompt is shown. You can also be
  # though of as being just after the last command, which allows access to the
  # exit status. Funny enough, this is the only way to access the exit status of
  # the last command.
  fzh_add_precmd_hook() {
    if [ -n "$FZH_DEBUG" ]; then
      print -u2 Exit code: $?
      print -u2 Cmd: $FZH_LAST_CMD
    fi

    $FZH_PATH add "$?:$FZH_LAST_CMD"
  }
  if [[ -z $precmd_functions ]] || [[ "${precmd_functions[(ie)fzh_add_precmd_hook]}" -gt ${#precmd_functions} ]]; then
    precmd_functions+=(fzh_add_precmd_hook)
  else
    [ -n "$FZH_DEBUG" ] && echo "fzh_add_precmd_hook already in precmd_functions, skipping"
  fi

  ##  Keybinds  ###############################################################

  if [[ $- =~ .*i.* ]]; then # Check it's an interactive shell
    fzh-widget() {
      () {
        # `BUFFER` is used to initialize with the contents of the terminal.

        echo "$(fzh search uno)"

        # selection=$($FZH_PATH search \""${BUFFER}"\")
        # eval "$selection"
        # eval "$($FZH_PATH search \"${BUFFER}\")"
      }
    }
    zle -N fzh-widget

    bindkey '^R' fzh-widget
  fi

fi
