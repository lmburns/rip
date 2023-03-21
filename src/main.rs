#![deny(clippy::all)]
#![warn(clippy::pedantic)]
#![allow(clippy::cast_possible_truncation)]

// TODO: add some tests

mod cli;
mod errors;
mod util;

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::os::unix::fs::{FileTypeExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::{env, fs, io};

use anstream::println;
use chrono::offset::Local;
use chrono::DateTime;
use clap::CommandFactory;
use clap_complete_command::Shell;
use cli::{BuryOpts, DecomposeOpts, RipCli, RipOptions, SeanceOpts, UnburyOpts};
use colored::Colorize;
use eyre::{bail, eyre, Result, WrapErr};
use globwalk::{GlobWalker, GlobWalkerBuilder};
use util::{
    humanize_bytes, join_absolute, parent_file_exists, prompt_yes, rename_grave, symlink_exists,
};
use walkdir::WalkDir;

// TODO: cleanup logging
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

/// Default graveyard location assuming that no other is assumed/specified
const GRAVEYARD: &str = "/tmp/graveyard";
/// Name of the record file
const RECORD: &str = ".record";
/// Default number of lines to print when inspecting (used in bury command)
const LINES_TO_INSPECT: usize = 6;
/// Default number of files to print when inspecting (used in bury command)
const FILES_TO_INSPECT: usize = 6;
/// Files above 500 MiB are considered large
const BIG_FILE_THRESHOLD: u64 = 500 * 1024 * 1024; // 500 MB
/// Max Depth for globbing. 10 because $HOME/.local/share/graveyard is already pretty deep
const DEFAULT_MAX_DEPTH: usize = 10;

struct RecordItem<'a> {
    _time: &'a str,
    orig: &'a Path,
    dest: &'a Path,
}

fn main() -> Result<()> {
    let (opts, color_preference) = RipOptions::init()?;

    anstream::force_color(color_preference);

    match opts {
        RipOptions::GenerateCompletions { shell } => completions_generate(shell),
        RipOptions::Bury(opts) => bury_command(opts),
        RipOptions::Decompose(opts) => decompose_graveyard(opts),
        RipOptions::Unbury(opts) => unbury(opts),
        RipOptions::Seance(opts) => seance_command(opts),
    }
}

fn decompose_graveyard(options: DecomposeOpts) -> Result<()> {
    let DecomposeOpts {
        graveyard,
        inspect: _,
        verbose,
    } = options;

    // TODO: print better log
    // TODO: if inspect, give stats about graveyard
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
                    file_type(&join_absolute(&graveyard, PathBuf::from(entry.orig),))
                        .bright_red()
                        .bold(),
                )?;
            }
            tab_handle.flush()?;
        }
        fs::remove_dir_all(graveyard).wrap_err("Couldn't unlink graveyard")?;
    }
    Ok(())
}

// TODO: FIX THIS
fn unbury(options: UnburyOpts) -> Result<()> {
    let UnburyOpts {
        graveyard,
        record,
        targets,
        cwd,
        max_depth,
        local,
        seance_opt,
        full_path,
        inspect: _,
        verbose,
    } = options;

    // Vector to hold the grave path of items we want to unbury.
    // This will be used to determine which items to remove from the
    // record following the unbury.
    // Allocate with at least the number of targets (assuming no globs)
    let mut graves_to_exhume = Vec::with_capacity(targets.len());

    for target in targets {
        // Detect if a glob exists
        if target.contains(['*', '?']) {
            let globbed = if local {
                glob_walk(&target, join_absolute(&graveyard, &cwd), max_depth)
            } else {
                glob_walk(&target, &graveyard, max_depth)
            };
            graves_to_exhume.extend(globbed);
        } else {
            let resolved_target = if local {
                join_absolute(join_absolute(&graveyard, &cwd), PathBuf::from(target))
            } else if target.starts_with(graveyard.to_str().unwrap()) {
                PathBuf::from(target)
            } else {
                join_absolute(&graveyard, PathBuf::from(target))
            };
            graves_to_exhume.push(resolved_target);
        }
    }

    let mut graves_to_exhume = dbg!(graves_to_exhume);

    if verbose {
        verbosed!("exhumed cli matches", graves_to_exhume);
    }
    // If -s is also passed, push all files found by seance onto
    // the graves_to_exhume.
    if seance_opt {
        if let Ok(f) = fs::File::open(&record) {
            let gravepath = join_absolute(&graveyard, cwd)
                .to_string_lossy()
                .into_owned();
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
        if local {
            if let Ok(s) = get_last_bury(&record, &new_cwd, "local") {
                if verbose {
                    verbose!("exhuming", "locally");
                }
                graves_to_exhume.push(s);
            }
        } else {
            if verbose {
                verbose!("exhuming", "globally");
            }
            if let Ok(s) = get_last_bury(&record, &new_cwd, "global") {
                graves_to_exhume.push(s);
            }
        }
        if verbose {
            verbosed!("exhumed last bury", graves_to_exhume);
        }
    }

    let graves_to_exhume = dbg!(graves_to_exhume);

    // Go through the graveyard and exhume all the graves
    let f = fs::File::open(&record).wrap_err("Couldn't read the record")?;
    for line in lines_of_graves(f, &graves_to_exhume) {
        let line = dbg!(line);
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
        if full_path {
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
    fs::File::open(&record)
        .and_then(|f| delete_lines_from_record(f, &record, &graves_to_exhume))
        .wrap_err(eyre!("Failed to remove unburied files from record."))
}

fn seance_command(options: SeanceOpts) -> Result<()> {
    let SeanceOpts {
        graveyard,
        show_all,
        full_path,
        plain,
        cwd,
        record,
    } = options;

    // If all is passed, list the entire graveyard
    let gravepath = if show_all {
        PathBuf::from(&graveyard)
    } else {
        join_absolute(&graveyard, &cwd)
    };

    let f = fs::File::open(&record)
        .wrap_err(format!("Failed to read record at {}", record.display()))?;
    let stdout = anstream::stdout();
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

        if full_path {
            if plain {
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

            if plain {
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
    Ok(())
}

fn bury_command(options: BuryOpts) -> Result<()> {
    let BuryOpts {
        graveyard,
        record,
        targets,
        cwd,
        inspect,
        verbose,
    } = options;

    for target in targets {
        // Check if source exists
        if let Ok(metadata) = fs::symlink_metadata(&target) {
            // Canonicalize the path unless it's a symlink
            let source = &if metadata.file_type().is_symlink() {
                cwd.join(&target)
            } else {
                cwd.join(&target)
                    .canonicalize()
                    .wrap_err("Failed to canonicalize path")?
            };

            if verbose {
                verbosed!("Resolved Target Path", source);
            }

            if inspect {
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
            if source.starts_with(&graveyard) {
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
                let dest = join_absolute(&graveyard, source);
                // Resolve a name conflict if necessary
                if symlink_exists(&dest) {
                    rename_grave(dest)
                } else if let Some(ancestor_file) = parent_file_exists(&dest) {
                    let new_ancestor = rename_grave(&ancestor_file);
                    let relative_dest = dest.strip_prefix(&ancestor_file).wrap_err_with(|| {
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
            write_log(source, dest, &record)
                .wrap_err_with(|| format!("Failed to write record at {}", record.display()))?;
        } else {
            bail!("Cannot remove {}: no such file or directory", target);
        }
    }
    Ok(())
}

/// Generate completions for a given shell. Color has no effect for completions
fn completions_generate(shell: Shell) -> Result<()> {
    let buffer = Vec::new();
    let mut cursor = Cursor::new(buffer);
    shell.generate(&mut RipCli::command(), &mut cursor);
    let buffer = cursor.into_inner();
    let script = String::from_utf8(buffer).wrap_err("Clap completion not UTF-8")?;

    println!("{}", script.trim());
    Ok(())
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
