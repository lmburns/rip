# Desc: fzf rip undo
# @dependencies: fzf

local -a undo
undo=( $(rip -sp | fzf) )

[[ -n $undo ]] && {
  for f ("$undo[@]") { rip -u "$f" }
} || builtin print -Pr "%F{2}Nothing undone%f"

# vim: ft=zsh:et:sw=0:ts=2:sts=2:fdm=marker:fmr={{{,}}}:
