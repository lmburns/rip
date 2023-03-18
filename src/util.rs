use colored::Colorize;
use std::env;
use std::fs;
use std::io;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

/// Concatenate two paths, even if the right argument is an absolute path.
pub(crate) fn join_absolute<A: AsRef<Path>, B: AsRef<Path>>(left: A, right: B) -> PathBuf {
    let (left, right) = (left.as_ref(), right.as_ref());
    left.join(if let Ok(stripped) = right.strip_prefix("/") {
        stripped
    } else {
        right
    })
}

pub(crate) fn symlink_exists<P: AsRef<Path>>(path: P) -> bool {
    fs::symlink_metadata(path).is_ok()
}

pub(crate) fn parent_file_exists<P: AsRef<Path>>(path: P) -> Option<PathBuf> {
    path.as_ref()
        .ancestors()
        .find(|ancestor| ancestor.is_file())
        .map(std::path::Path::to_path_buf)
}

pub(crate) fn get_user() -> String {
    env::var("USER").unwrap_or_else(|_| String::from("unknown"))
}

/// Prompt for user input, returning True if the first character is 'y' or 'Y'
pub(crate) fn prompt_yes<T: AsRef<str>>(prompt: T) -> bool {
    print!(
        "{} [{}/{}] ",
        prompt.as_ref(),
        "y".green().bold(),
        "N".red().bold()
    );
    if io::stdout().flush().is_err() {
        // If stdout wasn't flushed properly, fallback to println
        println!(
            "{} [{}/{}]",
            prompt.as_ref(),
            "y".green().bold(),
            "N".red().bold()
        );
    }
    let stdin = BufReader::new(io::stdin());
    stdin
        .bytes()
        .next()
        .and_then(std::result::Result::ok)
        .map(|c| c as char)
        .map_or(false, |c| (c == 'y' || c == 'Y'))
}

/// Add a numbered extension to duplicate filenames to avoid overwriting files.
pub(crate) fn rename_grave<G: AsRef<Path>>(grave: G) -> PathBuf {
    let grave = grave.as_ref();
    let name = grave.to_str().expect("Filename must be valid unicode.");
    (1_u64..u64::MAX)
        .map(|i| PathBuf::from(format!("{name}~{i}")))
        .find(|p| !symlink_exists(p))
        .expect("Failed to rename duplicate file or directory")
}

pub(crate) fn humanize_bytes(bytes: u64) -> String {
    let values = ["bytes", "KB", "MB", "GB", "TB"];
    let pair = values
        .iter()
        .enumerate()
        .take_while(|x| bytes as usize / 1000_usize.pow(x.0 as u32) > 10)
        .last();
    if let Some((i, unit)) = pair {
        format!("{} {}", bytes as usize / 1000_usize.pow(i as u32), unit)
    } else {
        format!("{} {}", bytes, values[0])
    }
}
