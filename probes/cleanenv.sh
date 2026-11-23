#!/bin/sh
# Run a command with a minimal env, like a desktop app would. Extra VAR=val can come first.
exec env -i HOME="$HOME" USER="$USER" PATH="$PATH" LANG="${LANG:-C.UTF-8}" SHELL=/bin/bash TERM=xterm-256color XDG_RUNTIME_DIR="$XDG_RUNTIME_DIR" "$@"
