#!/usr/bin/env bash
# R1782 — the transport this hook's own duration depends on.
#
# git opens the connection to the remote BEFORE it runs `pre-push`, so a hook
# that takes minutes leaves a socket idle for exactly that long. Measured
# across R1775-R1781: SEVEN consecutive pushes died `rc=141` (SIGPIPE) after
# every gate had PASSED. That is the worst shape a failure can take — the
# gates were green, the work was committed, and `git log` looked finished
# while `origin/main` had not moved.
#
# R1779 responded by deleting the two most expensive gates. R1782 put clippy
# back, having measured that it was never the expensive one (4.6s warm), which
# makes the keepalive LOAD-BEARING rather than incidental. And it was living
# in one shell's command line: at R1782 neither `core.sshCommand` nor
# `GIT_SSH_COMMAND` carried it in this clone, so every push was one forgotten
# environment variable away from the same seven failures.
#
# This file holds the decision as a pure function so it can be tested without
# a remote, a network or a push (`tools/test_hooks.sh`).

# keepalive_verdict <remote-url> <ssh-command>
#
# Answers `not-ssh`, `armed` or `missing` on stdout.
#
# ⚠ `ServerAliveInterval=0` is ssh's DEFAULT and means "never send one", so a
# check that merely greps for the option name calls the default armed. The
# interval is read as a number and must be positive. Both spellings ssh
# accepts are handled (`-o Name=value` and `-o "Name value"`).
#
# ⚠ What this CANNOT see: a keepalive set in `~/.ssh/config`, which is where
# it arguably belongs and which git never reports. That is why the caller's
# refusal carries an override rather than being absolute.
keepalive_verdict() {
    local url="${1:-}" ssh_cmd="${2:-}" interval

    case "$url" in
        git@* | ssh://*) ;;
        *)
            # https:// and file:// remotes do not open an ssh connection at
            # all, so there is nothing here for a keepalive to hold open.
            echo not-ssh
            return 0
            ;;
    esac

    interval="$(sed -n 's/.*ServerAliveInterval[= ]\{1,\}\([0-9]\{1,\}\).*/\1/p' <<<"$ssh_cmd")"
    if [[ -n "$interval" && "$interval" -gt 0 ]]; then
        echo armed
    else
        echo missing
    fi
}

# keepalive_configured_command
#
# What git will actually use, in git's own precedence: the environment beats
# the config. Kept beside the verdict so a caller cannot read one knob and
# forget the other — which is how this went unnoticed, since the seven
# failures were all pushed with the option typed on the command line as
# `GIT_SSH_COMMAND=...`, leaving `core.sshCommand` empty and nobody looking.
keepalive_configured_command() {
    if [[ -n "${GIT_SSH_COMMAND:-}" ]]; then
        printf '%s' "$GIT_SSH_COMMAND"
    else
        git config --get core.sshCommand 2>/dev/null || true
    fi
}
