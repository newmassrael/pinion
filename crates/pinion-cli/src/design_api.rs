//! R1614 §5.7 — the design service's wire constants, and the only place they
//! live.
//!
//! ## Why this module exists
//!
//! A standing directive says the names of the projects this one is judged
//! against must not appear in what we publish, and
//! `tools/reference_names.py` is the gate that keeps it true. The design-parity
//! workflow (R634-R643) is the one place where a vendor's name is **not** a
//! citation: it is an address. `api.figma.com` is where the request goes,
//! `X-Figma-Token` is the header that service reads, and `FIGMA_TOKEN` is the
//! environment variable a person exports before running the tool. None of the
//! three is ours to rename — renaming any of them stops the feature working.
//!
//! Everything that IS ours has been renamed to the role it plays: the CLI
//! sub-commands are `pinion design-verify` / `design-fetch-image` /
//! `design-diff`, the modules are `design_*`, and the reference binding is
//! `examples/design-button-m3`. What is left is exactly the protocol, and
//! putting it in one small module is what lets the gate exclude a **precise**
//! thing rather than a whole subsystem — the prose in the modules around this
//! one stays counted, so it cannot regrow a citation behind the exclusion.
//!
//! ## What a reader loses
//!
//! Nothing here: the constants below name the service plainly, because they
//! have to. What a reader loses is elsewhere — the prose no longer says which
//! design tool the parity loop was written against. That is recorded in this
//! project's memory notes, which are not part of the repository.

/// The REST host every request goes to.
pub const API_HOST: &str = "https://api.figma.com/v1";

/// The header the service reads the personal access token from.
pub const TOKEN_HEADER: &str = "X-Figma-Token";

/// The environment variable a person exports the token into.
pub const TOKEN_ENV: &str = "FIGMA_TOKEN";

/// Where a person gets a token, for the message shown when there is none.
pub const TOKEN_HELP: &str = "figma.com -> Settings -> Personal access tokens";

/// A file URL, for the `--help` example that shows where a file key is found.
pub const FILE_URL_EXAMPLE: &str = "https://www.figma.com/design/AbCdEfGhIj/My-Design";

/// The token read from [`TOKEN_ENV`], or the sentence to show when it is unset.
///
/// One statement of the auth contract, so the two sub-commands that need a
/// token cannot drift on how they ask for one.
///
/// # Errors
///
/// When the environment variable is unset or not valid UTF-8.
pub fn token() -> Result<String, String> {
    std::env::var(TOKEN_ENV).map_err(|_| {
        format!(
            "{TOKEN_ENV} environment variable not set; export \
             {TOKEN_ENV}=<personal access token> first ({TOKEN_HELP}, \
             scope: file read)"
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_auth_message_names_the_variable_a_person_has_to_set() {
        // The one thing a caller cannot guess: which variable. A message that
        // said "the token is missing" and not which name would be useless, and
        // this is the only place the name is written.
        let message = std::env::var(TOKEN_ENV)
            .err()
            .map(|_| token().expect_err("no token in a test environment"));
        if let Some(message) = message {
            assert!(message.contains(TOKEN_ENV), "{message}");
            assert!(message.contains(TOKEN_HELP), "{message}");
        }
    }

    #[test]
    fn the_host_carries_its_api_version() {
        // A version-less host would silently follow whatever the service makes
        // current; the pinned `v1` is what makes a response shape assertable.
        assert!(API_HOST.ends_with("/v1"), "{API_HOST}");
        assert!(API_HOST.starts_with("https://"), "{API_HOST}");
    }
}
