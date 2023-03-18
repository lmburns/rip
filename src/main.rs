#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

mod comp_helper;
mod errors;
mod util;

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use chrono::offset::Local;
use chrono::DateTime;
use clap::{crate_authors, crate_version, App, AppSettings, Arg};
use clap_generate::generators::{Bash, Elvish, Fish, PowerShell, Zsh};
use clap_generate::{generate, Generator};
use colored::Colorize;
use eyre::{bail, eyre, Result, WrapErr};
use globwalk::{GlobWalker, GlobWalkerBuilder};
use util::{
    get_user, humanize_bytes, join_absolute, parent_file_exists, prompt_yes, rename_grave,
    symlink_exists,
};
use walkdir::WalkDir;

macro_rules! fmt_exp {
    ($a:expr,$b:ident) => {
        $a.display().to_string().$b().bold()
    };
}

// These are somewhat treated as log::debug, etc
macro_rules! verbose {
    ($e:expr,$v:expr) => {
        println!(
            "{}: {}",
            $e.to_string().to_uppercase().green().bold(),
            $v.to_string().yellow()
        )
    };
}

macro_rules! verbosed {
    ($e:expr,$v:expr) => {
        println!(
            "{}: {:#?}",
            $e.to_string().to_uppercase().green().bold(),
            $v
        )
    };
}

const GRAVEYARD: &str = "/tmp/graveyard";
const RECORD: &str = ".record";
const LINES_TO_INSPECT: usize = 6;
const FILES_TO_INSPECT: usize = 6;
const BIG_FILE_THRESHOLD: u64 = 500_000_000; // 500 MB
const DEFAULT_MAX_DEPTH: usize = 10; // 10 because $HOME/.local/share/graveyard is already pretty deep

struct RecordItem<'a> {
    _time: &'a str,
    orig: &'a Path,
    dest: &'a Path,
}

enum Shell {
    Bash,
    Elvish,
    Fish,
    PowerShell,
    Zsh,
}

impl std::str::FromStr for Shell {
    type Err = eyre::ErrReport;

    fn from_str(s: &str) -> core::result::Result<Self, Self::Err> {
        match s.to_ascii_lowercase().trim() {
            "bash" => Ok(Shell::Bash),
            "elvish" => Ok(Shell::Elvish),
            "fish" => Ok(Shell::Fish),
            "powershell" => Ok(Shell::PowerShell),
            "zsh" => Ok(Shell::Zsh),
            _ => Err(eyre!("Invalid shell: {}", s.bright_red().bold())),
        }
    }
}

fn main() -> Result<()> {
    let matches = &cli_rip().get_matches();
    let nocolor: bool = matches.is_present("nocolor");
    let verbose: bool = matches.is_present("verbose");

    let graveyard: &PathBuf = &{
        if let Some(flag) = matches.value_of("graveyard") {
            flag.to_owned()
        } else if let Ok(env) = env::var("GRAVEYARD") {
            env
        } else if let Ok(mut env) = env::var("XDG_DATA_HOME") {
            if !env.ends_with(std::path::MAIN_SEPARATOR) {
                env.push(std::path::MAIN_SEPARATOR);
            }
            env.push_str("graveyard");
            env
        } else {
            format!("{}-{}", GRAVEYARD, get_user())
        }
    }
    .into();

    if verbose {
        verbose!("graveyard", graveyard.display());
    }

    if matches.is_present("decompose") {
        if prompt_yes("Really unlink the entire graveyard?") {
            if verbose {
                let stdout = io::stdout();
                let std_lock = stdout.lock();
                let handle = io::BufWriter::new(std_lock);
                let mut tab_handle = tabwriter::TabWriter::new(handle);

                writeln!(
                    tab_handle,
                    "{}\t{}",
                    "File".cyan().bold(),
                    "Type".bright_red().bold()
                )?;
                writeln!(
                    tab_handle,
                    "{}\t{}",
                    "----".cyan().bold(),
                    "----".bright_red().bold()
                )?;

                let mut f = fs::File::open(&graveyard.join(RECORD))?;
                let mut contents = String::new();
                f.read_to_string(&mut contents)?;

                for entry in contents.lines().map(record_entry) {
                    writeln!(
                        tab_handle,
                        "{}\t{}",
                        fmt_exp!(entry.orig, cyan),
                        file_type(&join_absolute(graveyard, PathBuf::from(entry.orig),))
                            .bright_red()
                            .bold(),
                    )?;
                }
                tab_handle.flush()?;
            }
            fs::remove_dir_all(graveyard).wrap_err("Couldn't unlink graveyard")?;
        }
        return Ok(());
    }

    let record: &Path = &graveyard.join(RECORD);
    let cwd: PathBuf = env::current_dir().wrap_err("Failed to get current dir")?;

    // == UNBURY ==
    if let Some(t) = matches.values_of("unbury") {
        // Maybe a cleaner way? This is to detect if a glob is given (*glob, **glob)
        let glob = t.clone().next().unwrap_or("None").contains('*');

        if verbose {
            verbose!("globbing", glob);
        }

        // Vector to hold the grave path of items we want to unbury.
        // This will be used to determine which items to remove from the
        // record following the unbury.
        // Initialize it with the targets passed to -r
        let graves_to_exhume = &mut {
            if glob {
                let max_d = if let Some(max_depth) = matches.value_of("max-depth") {
                    max_depth.parse::<usize>().unwrap()
                } else {
                    DEFAULT_MAX_DEPTH
                };
                if verbose {
                    verbose!("max depth", max_d);
                }

                if matches.is_present("local") {
                    glob_walk(
                        t.clone().next().unwrap(),
                        join_absolute(graveyard, &cwd),
                        max_d,
                    )
                } else {
                    glob_walk(t.clone().next().unwrap(), graveyard, max_d)
                }
            } else {
                // Match files in local directory
                if matches.is_present("local") {
                    t.clone()
                        .map(|file| {
                            join_absolute(join_absolute(graveyard, &cwd), PathBuf::from(file))
                        })
                        .collect::<Vec<PathBuf>>()
                } else if matches
                    .value_of("unbury")
                    .unwrap_or("None")
                    .to_string()
                    .contains(graveyard.to_str().unwrap())
                {
                    // Full path given (including graveyard)
                    t.clone().map(PathBuf::from).collect::<Vec<PathBuf>>()
                } else {
                    // Full path given (excluding graveyard, i.e., starting from $HOME)
                    t.clone()
                        .map(|file| join_absolute(graveyard, PathBuf::from(file)))
                        .collect::<Vec<PathBuf>>()
                }
            }
        };

        if verbose {
            verbosed!("exhumed cli matches", graves_to_exhume);
        }
        // If -s is also passed, push all files found by seance onto
        // the graves_to_exhume.
        if matches.is_present("seance") {
            if let Ok(f) = fs::File::open(record) {
                let gravepath = join_absolute(graveyard, cwd).to_string_lossy().into_owned();
                for grave in seance(f, gravepath) {
                    graves_to_exhume.push(grave);
                }
            }
            if verbose {
                verbosed!("exhumed after seance", graves_to_exhume);
            }
        }

        // Otherwise, add the last deleted file, globally or locally
        if graves_to_exhume.is_empty() {
            let new_cwd = env::current_dir().wrap_err("Failed to get current dir")?;
            if matches.is_present("local") {
                if let Ok(s) = get_last_bury(record, &new_cwd, "local") {
                    if verbose {
                        verbose!("exhuming", "locally");
                    }
                    graves_to_exhume.push(s);
                }
            } else {
                if verbose {
                    verbose!("exhuming", "globally");
                }
                if let Ok(s) = get_last_bury(record, &new_cwd, "global") {
                    graves_to_exhume.push(s);
                }
            }
            if verbose {
                verbosed!("exhumed last bury", graves_to_exhume);
            }
        }

        // Go through the graveyard and exhume all the graves
        let f = fs::File::open(record).wrap_err("Couldn't read the record")?;
        for line in lines_of_graves(f, graves_to_exhume) {
            let entry: RecordItem = record_entry(&line);
            let orig: &Path = &{
                if symlink_exists(entry.orig) {
                    rename_grave(entry.orig)
                } else {
                    PathBuf::from(entry.orig)
                }
            };
            bury(entry.dest, orig).wrap_err_with(|| {
                format!(
                    "Unbury failed: couldn't copy files from {} to {}",
                    fmt_exp!(entry.dest, magenta),
                    fmt_exp!(orig, red)
                )
            })?;
            // Replaces value of $GRAVEYARD with the variable name because it is so long
            if matches.is_present("fullpath") {
                println!(
                    "Returned {} to {}",
                    entry
                        .dest
                        .display()
                        .to_string()
                        .replace(graveyard.to_str().unwrap(), "$GRAVEYARD")
                        .magenta()
                        .bold(),
                    fmt_exp!(orig, red)
                );
            } else {
                println!("Returned {}", fmt_exp!(orig, red));
            }
        }

        // Reopen the record and then delete lines corresponding to exhumed graves
        if let Err(e) = fs::File::open(record)
            .and_then(|f| delete_lines_from_record(f, record, graves_to_exhume))
        {
            bail!("Failed to remove unburied files from record: {}", e);
        }
        return Ok(());
    }

    // == SEANCE ==
    if matches.is_present("seance") {
        // If all is passed, list the entire graveyard
        let gravepath = if matches.is_present("all") {
            PathBuf::from(graveyard)
        } else {
            join_absolute(graveyard, cwd)
        };

        let f = fs::File::open(record).wrap_err("Failed to read record")?;
        let stdout = io::stdout();
        let std_lock = stdout.lock();
        let handle = io::BufWriter::new(std_lock);
        let mut tab_handle = tabwriter::TabWriter::new(handle);

        for (i, grave) in seance(f, gravepath.to_string_lossy()).enumerate() {
            let metadata = fs::metadata(&grave);
            let created = match metadata.unwrap().clone().modified() {
                Ok(v) => {
                    let time: DateTime<Local> = v.into();
                    format!("{}", time.format("%Y-%m-%d %T")).to_string()
                }
                _ => "N/A".to_string(),
            };

            let otype = file_type(&grave);

            if nocolor {
                if matches.is_present("fullpath") {
                    if matches.is_present("plain") {
                        println!("{}", grave.display());
                    } else {
                        writeln!(
                            tab_handle,
                            "{}\t{}\t{}\t{}",
                            i,
                            created,
                            otype,
                            grave.display()
                        )?;
                    }
                } else {
                    let shortened = grave
                        .display()
                        .to_string()
                        .replace(graveyard.to_str().unwrap(), "");

                    if matches.is_present("plain") {
                        println!("{shortened}");
                    } else {
                        writeln!(tab_handle, "{i}\t{created}\t{otype}\t{shortened}")?;
                    }
                }
            } else {
                if matches.is_present("fullpath") {
                    if matches.is_present("plain") {
                        println!("{}", fmt_exp!(grave, yellow));
                    } else {
                        writeln!(
                            tab_handle,
                            "{}\t{}\t{:<5}\t{}",
                            i.to_string().green().bold(),
                            created.magenta().bold(),
                            otype.bright_red().bold(),
                            fmt_exp!(grave, yellow)
                        )?;
                    }
                } else {
                    let shortened = grave
                        .display()
                        .to_string()
                        .replace(graveyard.to_str().unwrap(), "")
                        .yellow()
                        .bold();

                    if matches.is_present("plain") {
                        println!("{shortened}");
                    } else {
                        writeln!(
                            tab_handle,
                            "{}\t{}\t{:<5}\t{}",
                            i.to_string().green().bold(),
                            created.magenta().bold(),
                            otype.bright_red().bold(),
                            shortened
                        )?;
                    }
                }
                tab_handle.flush()?;
            }
        }
        return Ok(());
    }

    if let Some(targets) = matches.values_of("TARGET") {
        for target in targets {
            // Check if source exists
            if let Ok(metadata) = fs::symlink_metadata(target) {
                // Canonicalize the path unless it's a symlink
                let source = &if metadata.file_type().is_symlink() {
                    cwd.join(target)
                } else {
                    cwd.join(target)
                        .canonicalize()
                        .wrap_err("Failed to canonicalize path")?
                };

                if matches.is_present("inspect") {
                    if metadata.is_dir() {
                        // Get the size of the directory and all its contents
                        println!(
                            "{}: directory, {} including:",
                            target.magenta().bold(),
                            humanize_bytes(
                                WalkDir::new(source)
                                    .into_iter()
                                    .filter_map(std::result::Result::ok)
                                    .filter_map(|x| x.metadata().ok())
                                    .map(|x| x.len())
                                    .sum::<u64>()
                            )
                            .green()
                            .bold()
                        );

                        // Print the first few top-level files in the directory
                        for entry in WalkDir::new(source)
                            .min_depth(1)
                            .max_depth(1)
                            .into_iter()
                            .filter_map(std::result::Result::ok)
                            .take(FILES_TO_INSPECT)
                        {
                            println!("{}", entry.path().display());
                        }
                    } else {
                        println!(
                            "{}: file, {}",
                            target.magenta().bold(),
                            humanize_bytes(metadata.len()).green().bold()
                        );
                        // Read the file and print the first few lines
                        if let Ok(f) = fs::File::open(source) {
                            for line in BufReader::new(f)
                                .lines()
                                .take(LINES_TO_INSPECT)
                                .filter_map(std::result::Result::ok)
                            {
                                println!("> {line}");
                            }
                        } else {
                            println!(
                                "{}: problem reading {}",
                                "Error".red().bold(),
                                fmt_exp!(source, magenta)
                            );
                        }
                    }
                    if !prompt_yes(format!(
                        "Send {} to the graveyard?",
                        target.magenta().bold()
                    )) {
                        continue;
                    }
                }

                // If rip is called on a file already in the graveyard, prompt
                // to permanently delete it instead.
                if source.starts_with(graveyard) {
                    println!(
                        "{} is already in the graveyard.",
                        source.display().to_string().magenta().bold()
                    );
                    if !prompt_yes("Permanently unlink it?") {
                        println!("Skipping {}", fmt_exp!(source, magenta));
                        return Ok(());
                    }

                    if fs::remove_dir_all(source).is_err() {
                        fs::remove_file(source).wrap_err("Couldn't unlink")?;
                    }
                }

                let dest: &Path = &{
                    let dest = join_absolute(graveyard, source);
                    // Resolve a name conflict if necessary
                    if symlink_exists(&dest) {
                        rename_grave(dest)
                    } else if let Some(ancestor_file) = parent_file_exists(&dest) {
                        let new_ancestor = rename_grave(&ancestor_file);
                        let relative_dest =
                            dest.strip_prefix(&ancestor_file).wrap_err_with(|| {
                                "Parent directory isn't a prefix of child directories?"
                            })?;
                        join_absolute(new_ancestor, relative_dest)
                    } else {
                        dest
                    }
                };

                bury(source, dest)
                    .map_err(|e| {
                        fs::remove_dir_all(dest).ok();
                        e
                    })
                    .wrap_err("Failed to bury file")?;
                // Clean up any partial buries due to permission error
                write_log(source, dest, record)
                    .wrap_err_with(|| format!("Failed to write record at {}", record.display()))?;
            } else {
                bail!("Cannot remove {}: no such file or directory", target);
            }
        }
    }

    if let Some(matches) = matches.subcommand_matches("completions") {
        let shell = matches.value_of("shell").unwrap().parse::<Shell>()?;

        let buffer = Vec::new();
        let mut cursor = Cursor::new(buffer);
        let mut app = cli_rip();

        match shell {
            Shell::Bash => print_completions::<Bash>(&mut app, &mut cursor),
            Shell::Elvish => print_completions::<Elvish>(&mut app, &mut cursor),
            Shell::Fish => print_completions::<Fish>(&mut app, &mut cursor),
            Shell::PowerShell => print_completions::<PowerShell>(&mut app, &mut cursor),
            Shell::Zsh => print_completions::<Zsh>(&mut app, &mut cursor),
        }

        let buffer = cursor.into_inner();
        let mut script = String::from_utf8(buffer).expect("Clap completion not UTF-8");

        // Modify the Zsh completions before printing them out
        match shell {
            Shell::Zsh => {
                for (needle, replacement) in comp_helper::ZSH_COMPLETION_REP {
                    replace(&mut script, needle, replacement)?;
                }
            }
            _ => println!(),
        }

        println!("{}", script.trim());
    }

    Ok(())
}

// cli interface
fn cli_rip() -> App<'static> {
    App::new("rip")
        .version(crate_version!())
        .author(crate_authors!())
        .setting(AppSettings::ArgRequiredElseHelp)
        .global_setting(AppSettings::ColoredHelp)
        .global_setting(AppSettings::ColorAuto)
        .about(
            "Rm ImProved
Send files to the graveyard ($XDG_DATA_HOME/graveyard if set, else /tmp/graveyard-$USER by \
             default) instead of unlinking them.",
        )
        .arg(
            Arg::new("TARGET")
                .about("File or directory to remove")
                .takes_value(true)
                .multiple(true),
        )
        .arg(
            Arg::new("graveyard")
                .about("Directory where deleted files go to rest")
                .long("graveyard")
                .short('G')
                .takes_value(true),
        )
        .arg(
            Arg::new("decompose")
                .about("Permanently deletes (unlink) the entire graveyard")
                .short('d')
                .long("decompose"),
        )
        .arg(
            Arg::new("seance")
                .about("Prints files that were sent under the current directory")
                .short('s')
                .long("seance"),
        )
        .arg(
            Arg::new("fullpath")
                .about("Prints full path of files under current directory (with -s)")
                .short('f')
                .long("fullpath"),
            // It'd be nice to have a requires for two other args using an or
            // seance, decompose
        )
        .arg(
            Arg::new("all")
                .about("Prints all files in graveyard")
                .short('a')
                .long("all")
                .requires("seance"),
        )
        // TODO: Use this everywhere
        .arg(
            Arg::new("nocolor")
                .about("Do not use colored output (in progress)")
                .short('N')
                .long("no-color"),
        )
        .arg(
            Arg::new("unbury")
                .about(
                    "Undo the last removal, or specify some file(s) in the graveyard. Can be \
                     glob, or combined with -s (see --help)",
                )
                .long_about(
                    "Undo last removal with no arguments, specify some files using globbing \
                     syntax, or combine with '-s' to undo all files that have been removed in \
                     current directory. Globbing syntax involves: *glob, **glob, *.{png,jpg,gif}, \
                     and using '!' before all previous mentioned globs to negate them. If '-l' is \
                     passed with no arguments, the most recently deleted file from '$CWD' will be \
                     returned.",
                )
                .short('u')
                .long("unbury")
                .value_name("target")
                .min_values(0),
        )
        .arg(
            Arg::new("max-depth")
                .about("Set max depth for glob to search (default: 10)")
                .short('m')
                .long("max-depth")
                .requires("unbury")
                .takes_value(true),
        )
        // TODO: use with glob
        .arg(
            Arg::new("local")
                .about("Undo files in current directory (local to current directory)")
                .long_about(
                    "Undo files that are in the current directory. If the files are in a \
                     directory below the directory that you are in, you have to specify that \
                     directory. For example if you're in a directory with a subdirectory 'src', \
                     and a file is in the $GRAVEYARD as $GRAVEYARD/$PWD/src/<file>, you must type \
                     'src/<file>' for it to be unburied. If a file is not specified, it will \
                     return the most recently deleted file from the local directory.",
                )
                .short('l')
                .long("local")
                .requires("unbury"),
        )
        .arg(
            Arg::new("plain")
                .about("Prints only file-path (to be used with scripts)")
                .long_about(
                    "Prints only file-path (that is: no index, no time). Can be used with any \
                     variation of '-sfpN'",
                )
                .short('p')
                .long("plain"),
        )
        .arg(
            Arg::new("inspect")
                .about("Prints some info about TARGET before prompting for action")
                .short('i')
                .long("inspect"),
        )
        .arg(
            Arg::new("verbose")
                .about("Print what is going on")
                .short('v')
                .long("verbose"),
        )
        .subcommand(
            App::new("completions")
                .version(crate_version!())
                .author(crate_authors!())
                .setting(AppSettings::Hidden)
                .about("AutoCompletion")
                .arg(
                    Arg::new("shell")
                        .short('s')
                        .long("shell")
                        .about("Selects shell")
                        .required(true)
                        .takes_value(true)
                        .possible_values(&["bash", "elvish", "fish", "powershell", "zsh"]),
                ),
        )
}

/// Print completions
pub fn print_completions<G: Generator>(app: &mut App, cursor: &mut Cursor<Vec<u8>>) {
    generate::<G, _>(app, app.get_name().to_string(), cursor);
}

/// Get the file's file type for displaying it
fn file_type(p: &Path) -> String {
    if fs::metadata(p).unwrap().is_file() {
        String::from("file")
    } else if fs::metadata(p).unwrap().is_dir() {
        String::from("dir")
    } else {
        String::from("other")
    }
}

/// Replace parts of completions output
fn replace(haystack: &mut String, needle: &str, replacement: &str) -> Result<()> {
    if let Some(index) = haystack.find(needle) {
        haystack.replace_range(index..index + needle.len(), replacement);
        Ok(())
    } else {
        Err(eyre!(
            "Failed to find text:\n{}\n…in completion script:\n{}",
            needle.to_string().red().bold(),
            (*haystack).green().bold(),
        ))
    }
}

/// Write deletion history to record
fn write_log<S, D, R>(source: S, dest: D, record: R) -> io::Result<()>
where
    S: AsRef<Path>,
    D: AsRef<Path>,
    R: AsRef<Path>,
{
    let (source, dest) = (source.as_ref(), dest.as_ref());
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record)?;
    let current_time = Local::now().format("%a %b %e %T %Y");
    writeln!(
        f,
        "{current_time}\t{src}\t{dest}",
        src = source.display(),
        dest = dest.display()
    )?;

    Ok(())
}

fn bury<S: AsRef<Path>, D: AsRef<Path>>(source: S, dest: D) -> Result<()> {
    let (source, dest) = (source.as_ref(), dest.as_ref());
    // Try a simple rename, which will only work within the same mount point.
    // Trying to rename across filesystems will throw errno 18.
    if fs::rename(source, dest).is_ok() {
        return Ok(());
    }

    // If that didn't work, then copy and rm.
    let parent = dest.parent().ok_or(eyre!("Couldn't get parent of dest"))?;
    fs::create_dir_all(parent).wrap_err("Couldn't create parent dir")?;

    if fs::symlink_metadata(source)
        .wrap_err("Couldn't get metadata")?
        .is_dir()
    {
        // for x in globwalk::glob() {
        // }
        // Walk the source, creating directories and copying files as needed
        for entry in WalkDir::new(source)
            .into_iter()
            .filter_map(std::result::Result::ok)
        {
            // Path without the top-level directory
            let orphan: &Path = entry
                .path()
                .strip_prefix(source)
                .wrap_err("Parent directory isn't a prefix of child directories?")?;
            if entry.file_type().is_dir() {
                fs::create_dir_all(dest.join(orphan)).wrap_err_with(|| {
                    format!(
                        "Failed to create {} in {}",
                        entry.path().display(),
                        dest.join(orphan).display()
                    )
                })?;
            } else {
                copy_file(entry.path(), dest.join(orphan)).wrap_err_with(|| {
                    format!(
                        "Failed to copy file from {} to {}",
                        entry.path().display(),
                        dest.join(orphan).display()
                    )
                })?;
            }
        }
        fs::remove_dir_all(source)
            .wrap_err_with(|| format!("Failed to remove dir: {}", source.display()))?;
    } else {
        copy_file(source, dest).wrap_err_with(|| {
            format!(
                "Failed to copy file from {} to {}",
                source.display(),
                dest.display()
            )
        })?;
        fs::remove_file(source)
            .wrap_err_with(|| format!("Failed to remove file: {}", source.display()))?;
    }

    Ok(())
}

fn copy_file<S: AsRef<Path>, D: AsRef<Path>>(source: S, dest: D) -> io::Result<()> {
    let (source, dest) = (source.as_ref(), dest.as_ref());
    let metadata = fs::symlink_metadata(source)?;
    let filetype = metadata.file_type();

    if metadata.len() > BIG_FILE_THRESHOLD {
        println!(
            "About to copy a big file ({} is {})",
            source.display(),
            humanize_bytes(metadata.len())
        );
        if prompt_yes("Permanently delete this file instead?") {
            return Ok(());
        }
    }

    if filetype.is_file() {
        fs::copy(source, dest)?;
    } else if filetype.is_fifo() {
        let mode = metadata.permissions().mode();
        std::process::Command::new("mkfifo")
            .arg(dest)
            .arg("-m")
            .arg(mode.to_string());
    } else if filetype.is_symlink() {
        let target = fs::read_link(source)?;
        std::os::unix::fs::symlink(target, dest)?;
    } else if let Err(e) = fs::copy(source, dest) {
        // Special file: Try copying it as normal, but this probably won't work
        println!("Non-regular file or directory: {}", source.display());
        if !prompt_yes("Permanently delete the file?") {
            return Err(e);
        }
        // Create a dummy file to act as a marker in the graveyard
        let mut marker = fs::File::create(dest)?;
        marker.write_all(
            b"This is a marker for a file that was \
                           permanently deleted.  Requiescat in pace.",
        )?;
    }

    Ok(())
}

/// Return the path in the graveyard of the last file to be buried.
/// As a side effect, any valid last files that are found in the record but
/// not on the filesystem are removed from the record.
fn get_last_bury<R>(record: R, cwd: &Path, cwdp: &str) -> io::Result<PathBuf>
where
    R: AsRef<Path>,
{
    let graves_to_exhume: &mut Vec<PathBuf> = &mut Vec::new();
    let mut f = fs::File::open(record.as_ref())?;
    let mut contents = String::new();
    f.read_to_string(&mut contents)?;

    for entry in contents.lines().rev().map(record_entry) {
        if cwdp == "local" {
            // If local and doesn't contain path to cwd, continue
            // Trying to exhume file that's not last bury globally, but locally
            if !entry.dest.to_str().unwrap().contains(cwd.to_str().unwrap()) {
                continue;
            } else if symlink_exists(entry.dest) {
                if !graves_to_exhume.is_empty() {
                    delete_lines_from_record(f, record, graves_to_exhume)?;
                }
                return Ok(PathBuf::from(entry.dest));
            }

            // File is gone, mark the grave to be removed from the record
            graves_to_exhume.push(PathBuf::from(entry.dest));
        } else if cwdp == "global" {
            // Check that the file is still in the graveyard.
            // If it is, return the corresponding line.
            if symlink_exists(entry.dest) {
                if !graves_to_exhume.is_empty() {
                    delete_lines_from_record(f, record, graves_to_exhume)?;
                }
                return Ok(PathBuf::from(entry.dest));
            }

            // File is gone, mark the grave to be removed from the record
            graves_to_exhume.push(PathBuf::from(entry.dest));
        }
    }

    if !graves_to_exhume.is_empty() {
        delete_lines_from_record(f, record, graves_to_exhume)?;
    }
    Err(io::Error::new(io::ErrorKind::NotFound, "But nobody came"))
}

/// Parse a line in the record into a `RecordItem`
fn record_entry(line: &str) -> RecordItem {
    let mut tokens = line.split('\t');
    let time: &str = tokens.next().expect("Bad format: column A");
    let orig: &str = tokens.next().expect("Bad format: column B");
    let dest: &str = tokens.next().expect("Bad format: column C");
    RecordItem {
        _time: time,
        orig: Path::new(orig),
        dest: Path::new(dest),
    }
}

/// Takes a vector of grave paths and returns the respective lines in the record
fn lines_of_graves(f: fs::File, graves: &[PathBuf]) -> impl Iterator<Item = String> + '_ {
    BufReader::new(f)
        .lines()
        .filter_map(std::result::Result::ok)
        .filter(move |l| graves.iter().any(|y| y == record_entry(l).dest))
}

/// Returns an iterator over all graves in the record that are under gravepath
fn seance<T: AsRef<str>>(f: fs::File, gravepath: T) -> impl Iterator<Item = PathBuf> {
    BufReader::new(f)
        .lines()
        .filter_map(std::result::Result::ok)
        .map(|l| PathBuf::from(record_entry(&l).dest))
        .filter(move |d| d.starts_with(gravepath.as_ref()))
}

/// Takes a vector of grave paths and removes the respective lines from the record
fn delete_lines_from_record<R: AsRef<Path>>(
    f: fs::File,
    record: R,
    graves: &[PathBuf],
) -> io::Result<()> {
    let record = record.as_ref();
    // Get the lines to write back to the record, which is every line except
    // the ones matching the exhumed graves.  Store them in a vector
    // since we'll be overwriting the record in-place.
    let lines_to_write: Vec<String> = BufReader::new(f)
        .lines()
        .filter_map(std::result::Result::ok)
        .filter(|l| !graves.iter().any(|y| y == record_entry(l).dest))
        .collect();
    let mut f = fs::File::create(record)?;
    for line in lines_to_write {
        writeln!(f, "{line}")?;
    }

    Ok(())
}

/// Create a `GlobWalkerBuilder` object that traverses the base directory, picking up
/// each file matching the pattern.
fn glob_walker<S>(base: S, pattern: S, max_depth: usize) -> eyre::Result<GlobWalker>
where
    S: AsRef<str>,
{
    let builder = GlobWalkerBuilder::new(base.as_ref(), pattern.as_ref());

    builder
        .max_depth(max_depth)
        .build()
        .map_err(|e| eyre!(e))
        .wrap_err("Invalid data")
}

/// Implement the `glob_walker` function, pushing each result to a Vec<PathBuf> and returning
/// this vector
fn glob_walk<P>(pattern: &str, base_path: P, max_depth: usize) -> Vec<PathBuf>
where
    P: AsRef<Path>,
{
    let mut globbed_paths: Vec<PathBuf> = Vec::new();
    let base_path = base_path.as_ref().to_string_lossy().to_string();

    for entry in glob_walker(base_path.as_str(), pattern, max_depth)
        .unwrap()
        .flatten()
    {
        globbed_paths.push(PathBuf::from(entry.path()));
    }

    globbed_paths
}
