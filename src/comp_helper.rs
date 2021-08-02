pub const ZSH_COMPLETION_REP: &[(&str, &str)] = &[
    (
        r#"'*::TARGET -- File or directory to remove:' \
":: :_rip_commands" \
"*::: :->rip" \"#,
        r#"'*::TARGET -- File or directory to remove:_files' \"#,
    ),
    (
        r#"    case $state in
    (rip)
        words=($line[2] "${words[@]}")
        (( CURRENT += 1 ))
        curcontext="${curcontext%:*:*}:rip-command-$line[2]:"
        case $line[2] in
            (completions)
_arguments "${_arguments_options[@]}" \
'-s+[Selects shell]: :(bash elvish fish powershell zsh)' \
'--shell=[Selects shell]: :(bash elvish fish powershell zsh)' \
'-h[Prints help information]' \
'--help[Prints help information]' \
'-V[Prints version information]' \
'--version[Prints version information]' \
&& ret=0
;;
(help)
_arguments "${_arguments_options[@]}" \
'-h[Prints help information]' \
'--help[Prints help information]' \
'-V[Prints version information]' \
'--version[Prints version information]' \
&& ret=0
;;
        esac
    ;;
esac"#,
    r#""#,
    ),
    (
        "(( $+functions[_rip_commands] )) ||
_rip_commands() {
    local commands; commands=(
        \"completions:AutoCompletion\" \\
\"help:Prints this message or the help of the given subcommand(s)\" \\
    )
    _describe -t commands 'rip commands' commands \"$@\"
}
(( $+functions[_rip__completions_commands] )) ||
_rip__completions_commands() {
    local commands; commands=(
\x20\x20\x20\x20\x20\x20\x20\x20
    )
    _describe -t commands 'rip completions commands' commands \"$@\"
}
(( $+functions[_rip__help_commands] )) ||
_rip__help_commands() {
    local commands; commands=(
\x20\x20\x20\x20\x20\x20\x20\x20
    )
    _describe -t commands 'rip help commands' commands \"$@\"
}

_rip \"$@\"",
        r#"_rip "$@""#
    )
];
