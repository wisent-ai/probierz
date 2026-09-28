//! What each read-only jeden view is for, as a person would describe it. The
//! routed model reads every view against these sentences; nothing here is
//! matched as text. These constants are the declared subject of each view.

/// Read-only slash commands and the subject matter their view must present.
pub(super) const VIEW_SUBJECTS: [(&str, &str); 28] = [
    (
        "/help",
        "the list of slash commands the application accepts, including how to pick a model",
    ),
    ("/hotkeys", "the keyboard shortcuts of the interface"),
    (
        "/context",
        "how the conversation's context window is used, measured in tokens",
    ),
    (
        "/status",
        "the session's current state: its capabilities, health and what it is allowed to do",
    ),
    (
        "/tools",
        "the tools the agent can call, such as reading and writing files or running commands",
    ),
    ("/prompt", "the system prompt the agent runs with"),
    (
        "/login",
        "the account or authentication the application is signed in with",
    ),
    (
        "/usage",
        "how much of the model quota or tokens this account has used",
    ),
    (
        "/roles",
        "the model roles the application assigns and which model serves each",
    ),
    ("/agents", "the agents available to the session"),
    ("/jobs", "the background jobs the session runs"),
    ("/session", "the current session: its identity and state"),
    (
        "/memory",
        "what the agent remembers across sessions and how that memory is indexed",
    ),
    ("/hooks", "the hooks that run around the agent's actions"),
    (
        "/extensions",
        "the extensions or plugins installed or discoverable",
    ),
    ("/plugins", "the installed plugins"),
    (
        "/marketplace",
        "the catalogue of plugins that can be installed",
    ),
    ("/mcp", "the MCP servers connected to the session"),
    ("/ssh", "the remote hosts reachable over SSH"),
    ("/browser", "the browser runtime the agent can drive"),
    ("/changelog", "the release history of the application"),
    ("/settings", "the application's settings, grouped by area"),
    (
        "/model",
        "the models the application can use and which one is selected",
    ),
    (
        "/approval",
        "when the agent must ask before acting, and what it may do without asking",
    ),
    (
        "/collab status",
        "the state of a collaboration session with other hosts or guests",
    ),
    ("/fast status", "whether fast mode is on"),
    ("/plan status", "the state of plan mode"),
    (
        "/stats",
        "usage statistics or a dashboard report of the session",
    ),
];

/// The judge answers deterministically.
pub(super) const TEMPERATURE: u8 = 0;
/// Output tokens the judge may spend on one verdict over every view.
pub(super) const MAX_OUTPUT_TOKENS: u64 = 4000;
/// The judge call gets three minutes.
pub(super) const ROUTER_BUDGET_SECONDS: u64 = 180;
/// The first-run screen a fresh home shows instead.
pub(super) const FIRST_RUN_SCREEN: &str = "Tips";
/// The first screen jeden paints once it is ready for input.
pub(super) const READY_SCREEN: &str = "Welcome back!";
/// How long jeden gets to start, and each view to paint, in seconds.
pub(super) const START_SECONDS: u64 = 30;
pub(super) const VIEW_SECONDS: u64 = 15;
