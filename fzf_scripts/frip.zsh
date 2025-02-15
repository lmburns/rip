# Desc: fzf rip remove files
# @dependencies: fzf, rip, fd (can be replaced)

local -a selection
local -A opts
local maxdepth type= exclude=

# d, directory, e, empty, executable, f, file, l, p, pipe, s, socket, symlink, x

zparseopts -D -A opts -- d: e: t:

(( $+opts[-d] )) && maxdepth=$opts[-d] || maxdepth=1
(( $+opts[-e] )) && exclude=$opts[-e]
(( $+opts[-t] )) && type=$opts[-t]

selection=( $(fd --color=always -d${maxdepth} ${exclude:+-E ${(b)exclude}} ${type:+-t${type}} \
  | fzf -m --prompt "Delete Files> ") )

[[ -n "$selection" ]] && rip -- "$selection[@]"

# vim: ft=zsh:et:sw=0:ts=2:sts=2:fdm=marker:fmr={{{,}}}:
