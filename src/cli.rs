use std::path::PathBuf;

use anstream::ColorChoice;
use clap::builder::PossibleValue;
use clap::{crate_authors, Parser, Subcommand, ValueEnum};
use clap_complete_command::Shell;
use eyre::{Result, WrapErr};

use crate::util::get_user;
use crate::{DEFAULT_MAX_DEPTH, GRAVEYARD, RECORD};

#[derive(Debug, Subcommand)]
// #[clap(hide = true)]
enum RipCommands {
    #[clap(hide = true)]
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Parser, Debug)]
#[command(author = crate_authors!(", "), version, about)]
#[command(
    long_about = "Send files to the graveyard ($XDG_DATA_HOME/graveyard if set, else \
                  /tmp/graveyard-$USER by default) instead of unlinking them."
)]
#[command(propagate_version = true)]
#[command(arg_required_else_help = true)]
#[command(help_template = "{before-help}{name} {version}
{author}
{about}

{usage-heading}
{tab}{usage}

{all-args}
{after-help}")]
pub struct RipCli {
    #[arg(
        help = "File or directory to send to the graveyard. Can be globbed",
        long_help = "One or more files or directories to send to the graveyard (or restored from \
                     the graveyard if combined with -u). Globbing syntax involves: '*glob', \
                     '**glob', '*.{png, jpg, gif}', and using '!' before all previous mentioned \
                     globs to negate them. "
    )]
    target: Vec<String>,

    #[arg(short = 'G', long, help = "Directory where deleted files go to rest")]
    graveyard: Option<PathBuf>,

    #[arg(
        short,
        long,
        help = "Undo the last removal, or restore some file(s) from the graveyard. Can be \
                combined with -s and -l",
        long_help = "Undo last removal with no TARGET, specify a TARGET using globbing syntax, or \
                     combine with '-s' and no TARGET to undo all files that have been removed in \
                     the current directory. Globbing with '-s' to undo all matching files that \
                     have been removed in the current directory. If '-l' is passed with no \
                     arguments, the most recently deleted file from '$CWD' is returned."
    )]
    unbury: bool,

    #[arg(short, long, help = "Set max depth for glob to search (default: 10)")]
    max_depth: Option<usize>,

    #[arg(
        short,
        long,
        help = "Permanently deletes (unlink) the entire graveyard"
    )]
    decompose: bool,

    #[arg(
        short,
        long,
        help = "Prints files that were sent under the current directory"
    )]
    seance: bool,

    #[arg(
        short,
        long,
        help = "Prints the full path of files under the current directory (with -s)"
    )]
    full_path: bool,

    #[arg(
        short = 'a',
        long = "all",
        help = "Prints all files in graveyard (with -s)",
        requires = "seance"
    )]
    show_all: bool,

    #[arg(
        short,
        long,
        help = "Undo files in current directory (local to current directory)",
        long_help = "Undo files that are in the current directory. If the files are in a \
                     directory below the directory that you are in, you have to specify that \
                     directory. For example if you're in a directory with a subdirectory 'src', \
                     and a file is in the $GRAVEYARD as $GRAVEYARD/$PWD/src/<file>, you must type \
                     'src/<file>' for it to be unburied. If a file is not specified, it will \
                     return the most recently deleted file from the local directory.",
        requires = "unbury"
    )]
    local: bool,

    #[arg(
        short,
        long,
        help = "Prints only the file-path (i.e. no index and no time). Can be used with any \
                variation of '-sfp'"
    )]
    plain: bool,

    #[arg(
        short,
        long,
        help = "Prints info about TARGET before prompting for action."
    )]
    inspect: bool,

    #[arg(short, long, help = "Print what is going on")]
    verbose: bool,

    // TODO: Clap currently does not do colors
    #[arg(
        short,
        long,
        help = "Select whether the output is colored", 
        id = "WHEN", 
        default_value = "auto", 
        aliases = ["colour"],
    )]
    color: ColorChoiceWrapper,

    /// Autocompletion
    #[command(subcommand)]
    subcommands: Option<RipCommands>,
}

#[derive(Debug, Clone)]
pub struct BuryOpts {
    pub graveyard: PathBuf,
    pub record: PathBuf,
    pub targets: Vec<String>,
    pub cwd: PathBuf,
    pub inspect: bool,
    pub verbose: bool,
}

#[derive(Debug, Clone)]
pub struct UnburyOpts {
    pub graveyard: PathBuf,
    pub record: PathBuf,
    pub targets: Vec<String>,
    pub cwd: PathBuf,
    pub max_depth: usize,
    pub local: bool,
    pub seance_opt: bool,
    pub full_path: bool,
    pub inspect: bool,
    pub verbose: bool,
}

#[derive(Debug, Clone)]
pub struct DecomposeOpts {
    pub graveyard: PathBuf,
    pub inspect: bool,
    pub verbose: bool,
}

#[derive(Debug, Clone)]
pub struct SeanceOpts {
    pub graveyard: PathBuf,
    pub record: PathBuf,
    pub cwd: PathBuf,
    pub show_all: bool,
    pub full_path: bool,
    pub plain: bool,
}

#[derive(Debug, Clone)]
pub enum RipOptions {
    GenerateCompletions { shell: Shell },
    Bury(BuryOpts),
    Decompose(DecomposeOpts),
    Unbury(UnburyOpts),
    Seance(SeanceOpts),
}

impl RipOptions {
    pub fn init() -> Result<(Self, ColorChoice)> {
        let args = RipCli::parse();

        // Automatically handles color preferences
        anstream::force_color(args.color.into());

        let graveyard: PathBuf = {
            if let Some(flag) = args.graveyard {
                flag
            } else if let Ok(env) = std::env::var("GRAVEYARD") {
                env.into()
            } else if let Ok(mut env) = std::env::var("XDG_DATA_HOME") {
                if !env.ends_with(std::path::MAIN_SEPARATOR) {
                    env.push(std::path::MAIN_SEPARATOR);
                }
                env.push_str("graveyard");
                env.into()
            } else {
                format!("{}-{}", GRAVEYARD, get_user()).into()
            }
        };
        let record: PathBuf = graveyard.join(RECORD);
        let cwd: PathBuf = std::env::current_dir().wrap_err("Failed to get current dir")?;
        let max_depth = if let Some(depth) = args.max_depth {
            depth
        } else {
            DEFAULT_MAX_DEPTH
        };

        let opts = {
            if let Some(subcommand) = args.subcommands {
                match subcommand {
                    RipCommands::Completions { shell } => Self::GenerateCompletions { shell },
                }
            } else if args.unbury {
                Self::Unbury(UnburyOpts {
                    graveyard,
                    record,
                    targets: args.target,
                    cwd,
                    max_depth,
                    local: args.local,
                    seance_opt: args.seance,
                    full_path: args.full_path,
                    inspect: args.inspect,
                    verbose: args.verbose,
                })
            } else if args.seance {
                Self::Seance(SeanceOpts {
                    graveyard,
                    cwd,
                    show_all: args.show_all,
                    full_path: args.full_path,
                    plain: args.plain,
                    record,
                })
            } else if args.decompose {
                Self::Decompose(DecomposeOpts {
                    graveyard,
                    inspect: args.inspect,
                    verbose: args.verbose,
                })
            } else {
                Self::Bury(BuryOpts {
                    graveyard,
                    record,
                    targets: args.target,
                    cwd,
                    inspect: args.inspect,
                    verbose: args.verbose,
                })
            }
        };

        match opts {
            // No color generation for completions
            Self::GenerateCompletions { .. } => Ok((opts, ColorChoice::Never)),
            _ => Ok((opts, args.color.into())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ColorChoiceWrapper {
    Auto,
    AlwaysAnsi,
    Always,
    Never,
}

impl ValueEnum for ColorChoiceWrapper {
    fn value_variants<'a>() -> &'a [Self] {
        &[Self::Auto, Self::AlwaysAnsi, Self::Always, Self::Never]
    }

    fn to_possible_value(&self) -> Option<clap::builder::PossibleValue> {
        Some(match self {
            ColorChoiceWrapper::Auto => PossibleValue::new("auto")
                .help("Automatically detect whether coloring should be used."),
            ColorChoiceWrapper::AlwaysAnsi => PossibleValue::new("ansi")
                .alias("always-ansi")
                .help("Always use colors. Only ANSI colors (not truecolor)."),
            ColorChoiceWrapper::Always => {
                PossibleValue::new("always").help("Always use color (true color).")
            }
            ColorChoiceWrapper::Never => {
                PossibleValue::new("never").help("Never use colored output.")
            }
        })
    }
}

impl From<ColorChoiceWrapper> for ColorChoice {
    fn from(val: ColorChoiceWrapper) -> Self {
        match val {
            ColorChoiceWrapper::Auto => ColorChoice::Auto,
            ColorChoiceWrapper::AlwaysAnsi => ColorChoice::AlwaysAnsi,
            ColorChoiceWrapper::Always => ColorChoice::Always,
            ColorChoiceWrapper::Never => ColorChoice::Never,
        }
    }
}
