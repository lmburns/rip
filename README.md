# rip (Rm ImProved)
[![crates.io](https://img.shields.io/crates/v/rm-improved.svg)](https://crates.io/crates/rm-improved)
[![travis-ci](https://travis-ci.org/nivekuil/rip.svg?branch=master)](https://travis-ci.org/nivekuil/rip)

#### Goals of Fork:
* [x]  Ability to list files under current directory with `$GRAVEYARD` removed. For example:
```sh
/Users/$USER/.local/share/graveyard/Users/$USER/test/colored
# Transforms to
/Users/$USER/test/colored
```

* [x] Ability to list all files that are removed (both with and without `$GRAVEYARD`) not just files under current directory.
3) Integrate `fzf` in a better way
* [x] Display timestamps
5) Maybe remove individual files from the `$GRAVEYARD` with glob
* [x] Better completion output (current do not work properly)
* [x] Use globs to return files, and prevent having to use full path
* [x] Ability to restore file in local directory by just mentioning file name

`rip` is a command-line deletion tool focused on safety, ergonomics, and performance.  It favors a simple interface, and does /not/ implement the `xdg-trash` spec or attempt to achieve the same goals.

Deleted files get sent to the graveyard (`$XDG_DATA_HOME/graveyard` if set, else `/tmp/graveyard-$USER` by default, see [notes](https://github.com/nivekuil/rip#-notes) on changing this) under their absolute path, giving you a chance to recover them.  No data is overwritten.  If files that share the same path are deleted, they will be renamed as numbered backups.

`rip` is made for lazy people.  If any part of the interface could be more intuitive, please open an issue or pull request.

## ⚰ Installation
#### Get a binary [release](https://github.com/nivekuil/rip/releases)
* Linux `x86_64`
* `ARMv7`
* `macOS`

`untar` it, and move it somewhere on your `$PATH`:

```sh
$ tar xvzf rip-*.tar.gz
$ mv rip /usr/local/bin
```

#### Or build it:
```sh
$ cargo install rm-improved
```

#### Installing this fork
```sh
$ git clone https://github.com/lmburns/rip
$ cd rip
$ cargo build --release && mv target/release/rip ~/bin
```

#### Arch Linux users can install it from the [AUR](https://aur.archlinux.org/packages/rm-improved/) (thanks `@Charon77`!)
```sh
$ yay -S rm-improved
```

#### `macOS` users can install it with Homebrew:

```sh
$ brew install rm-improved
```

## ⚰ Usage

```sh
USAGE:
    rip [FLAGS] [OPTIONS] [TARGET]...

ARGS:
    <TARGET>...    File or directory to remove

FLAGS:
    -a, --all          Prints all files in graveyard
    -d, --decompose    Permanently deletes (unlink) the entire graveyard
    -f, --fullpath     Prints full path of files under current directory (with -s)
    -h, --help         Prints help information
    -i, --inspect      Prints some info about TARGET before prompting for action
    -l, --local        Undo files in current directory (local to current directory)
    -N, --no-color     Do not use colored output (in progress)
    -p, --plain        Prints only file-path (to be used with scripts)
    -s, --seance       Prints files that were sent under the current directory
    -v, --verbose      Print what is going on
    -V, --version      Prints version information

OPTIONS:
    -G, --graveyard <graveyard>    Directory where deleted files go to rest
    -m, --max-depth <max-depth>    Set max depth for glob to search (default: 10)
    -u, --unbury <target>          Undo the last removal, or specify some file(s) in the graveyard.
                                   Can be glob, or combined with -s (see --help)
```

#### Basic usage -- easier than `rm`
```sh
$ rip dir1/ file1
```

#### Undo the last deletion
```sh
$ rip -u
Returned /Users/jack/.local/share/graveyard/Users/jack/file1 to /Users/jack/file1
```

#### Print some info
Print the size and first few lines in a file, total size and first few files in a directory, and then prompt for deletion
```sh
$ rip -i file1
dir1: file, 1337 bytes including:
   >Position: Shooting Guard and Small Forward ▪ Shoots: Right
   >6-6, 185lb (198cm, 83kg)
Send file1 to the graveyard? (y/n) y
```

#### Print files that were deleted
These two options can be used with `-p` to prevent displaying index and time, and/or `-N` to not display colored output.

##### Shortened path to buried file (by default, under current directory)
```sh
$ rip -s
0  - [2021-07-31 16:40:45] /Users/jack/file1
1  - [2021-07-31 18:21:23] /Users/jack/dir1
```

##### Full path to buried file (under current directory)
```sh
$ rip -sf
0  - [2021-07-31 16:40:45] /Users/jack/.local/share/graveyard-jack/Users/jack/file1
1  - [2021-07-31 18:21:23] /Users/jack/.local/share/graveyard-jack/Users/jack/dir1
```

##### Shortened path to buried file (all)
```sh
$ rip -sa
0  - [2021-07-31 16:40:45] /Users/jack/file1
1  - [2021-07-31 18:21:23] /Users/jack/dir1
2  - [2021-07-31 18:48:49] /usr/local/share/dir1
3  - [2021-07-31 19:09:41] /usr/local/share/dir2
```

#### Name conflicts are resolved
```sh
$ touch file1
$ rip file1
$ rip -s
0  - [2021-07-31 16:40:45] /Users/jack/file1
1  - [2021-07-31 18:21:23] /Users/jack/dir1
2  - [2021-07-31 18:22:34] /Users/jack/file1~1
```

#### `-u` also takes the path of a file in the `graveyard`
##### Full path (including `$GRAVEYARD`)
This option is mainly here for compatibility with scripts or anything else that uses older versions.
```sh
$ rip -u /Users/jack/.local/share/graveyard-jack/Users/jack/file1
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/file1 to /Users/jack/file1
```

##### Full path (from `$HOME`)
```sh
$ rip -u /Users/jack/file1
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/file1 to /Users/jack/file1
```

##### File `l`ocal to current directory
```sh
$ rip -s
0  - [2021-07-31 16:40:45] /Users/jack/folder/folder2/file1
$ pwd
/Users/jack/folder
$ rip -lu folder2/file1
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/folder/folder2/file1 to /Users/jack/folder/folder2/file1
```

##### A glob pattern
A glob is detected if an asterisk is used. A max-depth can be specified using `-m` or `--max-depth`
```sh
$ rip -s
0  - [2021-07-31 16:40:45] /Users/jack/folder/folder2/file1
1  - [2021-07-31 18:21:23] /Users/jack/dir1
2  - [2021-07-31 18:22:34] /Users/jack/file2

$ rip -u '*file*'
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/folder/folder2/file1 to /Users/jack/folder/folder2/file1
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/file2 to /Users/jack/file2
```

**NOTE:** Glob patterns can consist of:
    * `*glob`, `!*glob`
    * `**glob`, `!**glob` - Traverse many directories (however, `--max-depth` should probably be used for this)
    * `*.{png.jpg,jpeg}`, `!*.{png.jpg,jpeg}` - Multiple patterns
    * The `!` negates the pattern

#### Combine `-u` and `-s` to restore everything printed by `-s`
```sh
$ rip -su
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/dir1 to /Users/jack/dir1
Returned /Users/jack/.local/share/graveyard-jack/Users/jack/file1~1 to /Users/jack/file1~1
```

### Emacs
```emacs
(setq delete-by-moving-to-trash t)
(defun system-move-file-to-trash (filename)
  (shell-command (concat (executable-find "rip") " " filename)))
```

## ⚰ Notes
- You probably shouldn't alias `rm` to `rip`.
    - Unlearning muscle memory is hard, but it's harder to ensure that every `rm` you make (as different users, from different machines and application environments) is the aliased one.
    - If you're using `zsh`, it is possible to create a `zsh` function like the following and add it to your `fpath`. This allows the user to use `rm` as `rip`, but if you type `sudo`, then it will use the actual `rm` command.
```zsh
[[ $EUID -ne 0 ]] && rip "${@}" || command rm -I -v "${@}"
```

- If you have `$XDG_DATA_HOME` environment variable set, `rip` will use `$XDG_DATA_HOME/graveyard` instead of the `/tmp/graveyard-$USER`.
- If you want to put the graveyard somewhere else (like `~/.local/share/Trash`), you have two options, in order of precedence:
```zsh
# 1) Aliasing rip
alias rip="rip --graveyard $HOME/.local/share/Trash"

# 2) Set environment variable
export GRAVEYARD="$HOME/.local/share/Trash"
```
 This can be a good idea because if the `graveyard` is mounted on an in-memory filesystem (as `/tmp` is in Arch Linux), deleting large files can quickly fill up your RAM.  It's also much slower to move files across file-systems, although the delay should be minimal with an SSD.

- In general, a deletion followed by a `--unbury` should be idempotent.
- The deletion log is kept in `.record`, found in the top level of the graveyard.
