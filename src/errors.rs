error_chain! {
    foreign_links {
       IoError(::std::io::Error);
      }

    errors {
        MismatchedCompletion(n: colored::ColoredString, h: colored::ColoredString) {
            description("unable to find completion text matching clap::Shell")
            display(
                "Failed to find text:\n{}\n…in completion script:\n{}",
                n, h
            )
        }

        InvalidShell(s: colored::ColoredString) {
            description("invalid shell entered"),
            display("Invalid shell: {}", s)
        }
    }
}
