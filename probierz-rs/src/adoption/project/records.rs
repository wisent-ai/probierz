use crate::adoption::*;
pub(crate) const INDEX_SCHEMA: &str = "ai.wisent.probierz.project-adoptions.v1";
pub(crate) const RESULT_SCHEMA: &str = "ai.wisent.probierz.project-adoption-result.v1";
pub(crate) const INDEX_RELATIVE_PATH: &str = "apps/.adoptions.json";
pub(crate) const SPEC_DIRECTORIES: [&str; 3] = ["test/specs", "tests", "specs"];
pub(crate) const TARGET_PACKAGES: [(&str, &str); 9] = [
    ("web", "packages/web"),
    ("electron", "packages/electron"),
    ("mobile:ios", "packages/mobile"),
    ("mobile:ios:byk-auth", "packages/mobile"),
    ("mobile:android", "packages/mobile"),
    ("desktop:mac", "packages/desktop-native"),
    ("desktop:win", "packages/desktop-native"),
    ("desktop:cua", "packages/desktop-cua"),
    ("tui", "packages/tui"),
];

pub const ONBOARDING_HELP: &str = "\
Accepted arguments:
  --reset                  discard saved walkthrough progress
  --source <repository>    adopt definitions from this existing Git repository
  --replace                replace reviewed conflicts; requires --source
  --json                   print the journey and adoption result as JSON

Defaults: keep walkthrough progress, adopt no source, preserve existing files, and render text.";

pub const PROJECT_HELP: &str = "\
Operations:
  adopt       validate and persist definitions without running them
  adoptions   list retained source identities";

#[derive(Debug, Subcommand)]
pub enum ProjectCommand {
    /// Adopt existing application manifests and journey specs without running them.
    #[command(override_usage = "probierz project adopt --source <repository> [--replace]")]
    Adopt {
        /// Existing Git repository whose definitions should be adopted.
        #[arg(long, value_name = "repository")]
        source: Option<PathBuf>,
        /// Replace reviewed unmanaged or unchanged same-source definitions.
        #[arg(long)]
        replace: bool,
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
    /// List the retained identities of adopted definition sources.
    #[command(override_usage = "probierz project adoptions")]
    Adoptions {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, hide = true)]
        args: Vec<String>,
    },
}

#[derive(Clone)]
pub(crate) struct DefinitionFile {
    pub(crate) relative: String,
    pub(crate) source: PathBuf,
    pub(crate) mode: u32,
    pub(crate) bytes: Vec<u8>,
    pub(crate) sha256: String,
}

pub(crate) struct Definitions {
    pub(crate) application_ids: Vec<String>,
    pub(crate) files: Vec<DefinitionFile>,
    pub(crate) skipped_local_state: Vec<String>,
    pub(crate) source_digest: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RetainedFile {
    pub(crate) path: String,
    pub(crate) sha256: String,
    pub(crate) mode: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SourceRecord {
    pub(crate) source_key: String,
    pub(crate) source_root: String,
    #[serde(default)]
    pub(crate) source_digest: String,
    #[serde(default)]
    pub(crate) adopted_at: String,
    #[serde(default)]
    pub(crate) applications: Vec<String>,
    pub(crate) files: Vec<RetainedFile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AdoptionIndex {
    pub(crate) schema: String,
    pub(crate) sources: Vec<SourceRecord>,
}

#[derive(Clone)]
pub(crate) struct Conflict {
    pub(crate) path: String,
    pub(crate) reason: &'static str,
    pub(crate) existing_sha256: Value,
    pub(crate) incoming_sha256: Value,
}

pub(crate) struct Counts {
    pub(crate) imported: usize,
    pub(crate) unchanged: usize,
    pub(crate) removed: usize,
    pub(crate) rejected: usize,
}

pub fn dispatch(project_root: &Path, command: ProjectCommand) -> Answer {
    match command {
        ProjectCommand::Adopt {
            source,
            replace,
            args,
        } => {
            if let Some(value) = args.first() {
                if value.starts_with("--") {
                    invocation_error(format!("unknown project adoption option: {value}"));
                }
                invocation_error(format!("unexpected project adoption argument: {value}"));
            }
            let Some(source) = source else {
                invocation_error("project adopt needs --source <repository>");
            };
            let result = adopt_project(project_root, &source, replace)?;
            let accepted = result.get("status").and_then(Value::as_str) != Some("conflict");
            print_json(&result)?;
            if !accepted {
                std::process::exit(1);
            }
            Ok(())
        }
        ProjectCommand::Adoptions { args } => {
            if !args.is_empty() {
                invocation_error("project adoptions accepts no options");
            }
            print_json(&list_project_adoptions(project_root)?)
        }
    }
}

pub(crate) fn invocation_error(detail: impl Into<String>) -> ! {
    clap::Error::raw(clap::error::ErrorKind::InvalidValue, detail.into()).exit()
}

