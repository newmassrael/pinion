#!/usr/bin/env bash
# Shared commit-message linter for the pinion githooks.
#
# `lint_commit_message <msg_file>` validates ONE commit message against
# COMMIT_FORMAT.md. It prints violations to stderr and returns 0 (conforms)
# or 1 (violations). It is the single source of truth for the rules, sourced
# by BOTH:
#
#   * `commit-msg` — validates the message being written (commit time), and
#   * `pre-push`   — re-validates EVERY commit being pushed (publish time),
#                    so a `--no-verify` bypass of `commit-msg` (or an amend /
#                    rebase that reintroduces a violation) is still caught
#                    before the commit reaches a remote.
#
# All state is function-local so `pre-push` can call it in a loop over many
# commits without one commit's parse leaking into the next.
#
# Rules (see COMMIT_FORMAT.md):
#   1. Subject (line 1): `<type>(<scope>)?: <subject>` shape; type in
#      {feat, fix, refactor, test, docs, build, chore}; max 72 bytes;
#      no trailing period.
#   2. Line 2: blank (separator).
#   3. Body lines: max 72 bytes each; ONE BULLET = ONE LINE (no indented /
#      wrapped continuation). Body is `- ` bullets ONLY — no prose lead
#      paragraph, no blank line between bullets (contiguous) — and at most
#      3 bullets (COMMIT_FORMAT.md §3).
#   4. Style: English only (ASCII U+0020-U+007E plus the small typographic
#      whitelist §, –, —, •, …, →); no `Co-Authored-By`; no `Generated with
#      Claude Code`; no emoji (Unicode pictograph ranges U+1F300-U+1FAFF and
#      U+1F1E6-U+1F1FF).

lint_commit_message() {
    local msg_file="$1"

    # Skip merge commits — git writes their subject and the user did not
    # author it.
    local first_line
    first_line=$(head -n 1 "$msg_file")
    if [[ "$first_line" =~ ^Merge ]]; then
        return 0
    fi

    # Strip leading '#'-comment lines (git appends instructions during an
    # interactive commit; they are stripped before the commit lands, so the
    # linter should not see them as content).
    local -a lines
    mapfile -t lines < <(grep -v '^#' "$msg_file")

    local -a errors=()

    # Last non-empty line index — trim trailing blank lines.
    local last_nonempty=-1
    local i
    for ((i=${#lines[@]}-1; i>=0; i--)); do
        if [[ -n "${lines[i]}" ]]; then
            last_nonempty=$i
            break
        fi
    done

    if [[ "$last_nonempty" -lt 0 ]]; then
        echo "commit-msg: empty message" >&2
        return 1
    fi

    # --- Rule 1: Subject line (line 1) ---
    local subject="${lines[0]:-}"
    local subject_bytes
    subject_bytes=$(printf '%s' "$subject" | wc -c)

    if ! [[ "$subject" =~ ^(feat|fix|refactor|test|docs|build|chore)(\([a-zA-Z0-9_/-]+\))?:\ .+$ ]]; then
        errors+=("Line 1: subject must match '<type>(<scope>)?: <subject>' with type in {feat,fix,refactor,test,docs,build,chore}")
    fi

    if [[ "$subject_bytes" -gt 72 ]]; then
        errors+=("Line 1: $subject_bytes bytes (max 72)")
    fi

    if [[ "$subject" =~ \.$ ]]; then
        errors+=("Line 1: subject must not end with a period")
    fi

    # --- Rule 2: Line 2 must be blank (when body is present) ---
    if [[ "$last_nonempty" -ge 1 && -n "${lines[1]:-}" ]]; then
        errors+=("Line 2: must be blank (separator between subject and body)")
    fi

    # --- Rule 3: Body lines (lines 3+) ---
    # Body is `- ` bullets only — no prose lead paragraph — at most 3, and
    # ONE BULLET = ONE LINE (a wrapped/indented continuation is rejected).
    local prev_was_bullet=0
    local bullet_count=0
    local line line_num line_bytes
    for ((i=2; i<=last_nonempty; i++)); do
        line="${lines[i]:-}"
        line_num=$((i+1))

        # No blank / whitespace-only line inside the body — the bullets
        # must be contiguous (a blank "enter" between bullets is rejected).
        # COMMIT_FORMAT.md §3.
        if [[ "$line" =~ ^[[:space:]]*$ ]]; then
            errors+=("Line $line_num: blank line inside body — bullets must be contiguous, no blank separators (COMMIT_FORMAT.md §3)")
            prev_was_bullet=0
            continue
        fi

        line_bytes=$(printf '%s' "$line" | wc -c)

        # Length check
        if [[ "$line_bytes" -gt 72 ]]; then
            errors+=("Line $line_num: $line_bytes bytes (max 72)")
        fi

        # Bullet-only: every non-empty body line must start with '- '.
        # The indented-continuation case (a wrapped bullet) gets a more
        # specific message; any other non-bullet line is a prose paragraph.
        if [[ "$line" =~ ^-\  ]]; then
            prev_was_bullet=1
            bullet_count=$((bullet_count+1))
        else
            if [[ "$prev_was_bullet" -eq 1 && "$line" =~ ^[[:space:]]+[^[:space:]] ]]; then
                errors+=("Line $line_num: indented continuation of previous bullet — one bullet = one line (COMMIT_FORMAT.md §3)")
            else
                errors+=("Line $line_num: body must be '- ' bullets only, no prose (COMMIT_FORMAT.md §3)")
            fi
            prev_was_bullet=0
        fi
    done

    # At most 3 bullets — condense to the key changes (COMMIT_FORMAT.md §3).
    if [[ "$bullet_count" -gt 3 ]]; then
        errors+=("Body has $bullet_count bullets (max 3) — condense to 1-3 key changes (COMMIT_FORMAT.md §3)")
    fi

    # --- Rule 4: Forbidden style markers (whole message) ---
    if grep -q -E 'Co-Authored-By|Co-authored-by' "$msg_file"; then
        errors+=("Forbidden tag: 'Co-Authored-By' (COMMIT_FORMAT.md §4)")
    fi
    if grep -q -E 'Generated with Claude Code|Generated by Claude Code' "$msg_file"; then
        errors+=("Forbidden tag: 'Generated with Claude Code' (COMMIT_FORMAT.md §4)")
    fi

    # Emoji detection — Unicode pictograph / regional-indicator ranges.
    # Typographic symbols (§ U+00A7, → U+2192, – U+2013, • U+2022, etc.) are
    # below the U+1F300 cutoff so they are not flagged.
    if grep -qP '[\x{1F300}-\x{1FAFF}\x{1F1E6}-\x{1F1FF}]' "$msg_file" 2>/dev/null; then
        errors+=("Emoji detected (COMMIT_FORMAT.md §4: no emojis)")
    fi

    # English-only check — whitelist ASCII printable (U+0020-U+007E) + tab
    # (U+0009) + LF (U+000A) + CR (U+000D) plus the small typographic
    # symbol set the commit log is allowed to carry (§ U+00A7, en-dash
    # U+2013, em-dash U+2014, bullet U+2022, ellipsis U+2026, rightwards
    # arrow U+2192). Anything else (Hangul, Hiragana, Katakana, CJK
    # ideographs, Cyrillic, Greek, etc.) is rejected.
    #
    # Why a whitelist rather than denylist: the commit log is a long-lived
    # audit trail consumed by every project collaborator; mixing scripts
    # turns log readers into Unicode parsers. Project narrative that genuinely
    # needs Korean (round summaries, SEED prompt, auto-memory) lives in
    # in-tree files where the audience expects it.
    local non_english_lines offender
    if non_english_lines=$(grep -nP '[^\x{0009}\x{000A}\x{000D}\x{0020}-\x{007E}\x{00A7}\x{2013}\x{2014}\x{2022}\x{2026}\x{2192}]' "$msg_file" 2>/dev/null); then
        if [[ -n "$non_english_lines" ]]; then
            errors+=("Non-English / non-whitelisted character (COMMIT_FORMAT.md §4: English only)")
            # Surface the first few offending lines so the author can locate
            # the exact characters without re-reading the message.
            while IFS= read -r offender; do
                errors+=("  $offender")
            done <<< "$(printf '%s\n' "$non_english_lines" | head -n 5)"
        fi
    fi

    if [[ "${#errors[@]}" -gt 0 ]]; then
        echo "commit-msg: COMMIT_FORMAT.md violations:" >&2
        local err
        for err in "${errors[@]}"; do
            echo "  - $err" >&2
        done
        echo "" >&2
        echo "See COMMIT_FORMAT.md for the full rule set." >&2
        return 1
    fi

    return 0
}

# R1760 — the round token a subject declares, or empty.
#
# `feat(rpc): R1757 a burst ...` -> `R1757`
# `fix(runtime): R1753.1 a count ...` -> `R1753.1`
#
# A CONTINUATION IS A DIFFERENT TOKEN FROM ITS PARENT, deliberately and by
# construction: 106 commits in this history are `.N` follow-ups to a round
# that is already committed, so a check that folded `R1753.1` onto `R1753`
# would refuse every one of them. Comparing whole tokens keeps continuations
# legal without a special case.
#
# Pure — takes the subject as an argument and calls no git — so
# `tools/test_hooks.sh` drives every arm.
round_token_of() {
    grep -oE '\bR[0-9]+(\.[0-9]+)?\b' <<<"${1:-}" | head -1 || true
}

# R1760 — is `token` already carried by a commit subject in `history`?
#
# `history` is passed in (newline-separated subjects) rather than read here,
# for the same reason: the decision is testable without a repository.
#
# ## Why this gate exists
#
# `git log` cannot see a round that has not committed yet, so two sessions
# starting the same afternoon derive the same next number. Measured
# 2026-08-21: R1757 and R1758 were both begun as "R1757", and the collision
# surfaced only because one session happened to read the other's memory file —
# after which 85 sites were renumbered. R1759 made `worktree.sh` CLAIM a
# number, which fixes it for anyone using that tool and is therefore advisory.
# This is the backstop: the first moment a duplicate is knowable from git is
# the second commit, and this refuses it there.
round_token_taken() {
    local token="${1:-}" history="${2:-}"
    [[ -n $token ]] || return 1
    grep -qE "(^|[^0-9A-Za-z.])${token//./\\.}([^0-9.]|$)" <<<"$history"
}
