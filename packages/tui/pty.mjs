// PTY driver for TUI specs. Uses python3's stdlib pty.spawn to give the
// child process a real controlling terminal (zero dependencies), forwards
// keystrokes on stdin, and exposes the current screen (last repaint frame)
// plus the whole session log with ANSI escapes stripped.
import { spawn } from "node:child_process";

const KEYS = {
  enter: "\r",
  tab: "\t",
  esc: "\x1b",
  backspace: "\x7f",
  "ctrl-c": "\x03",
  "ctrl-d": "\x04",
  up: "\x1b[A",
  down: "\x1b[B",
  right: "\x1b[C",
  left: "\x1b[D",
};

const ANSI_PATTERNS = [
  /\x1b\][^\x07]*(?:\x07|\x1b\\)/g,
  /\x1b\[[0-9;?]*[ -/]*[@-~]/g,
  /\x1b[()][0-9A-B]/g,
  /\x1b[=>]/g,
];
const CLEAR_SCREEN = /\x1b\[(?:2|3)?J|\x1b\[H\x1b\[(?:2|3)?J/;

export function stripAnsi(text) {
  let out = String(text);
  for (const pattern of ANSI_PATTERNS) out = out.replace(pattern, "");
  return out.replace(/\r/g, "");
}

export function spawnTui(command, args = [], { cwd, env, cols = 120, rows = 36 } = {}) {
  const shim = `stty rows ${rows} cols ${cols}; exec "$@"`;
  const child = spawn("python3", ["-c", "import os,pty,sys;sys.exit(os.waitstatus_to_exitcode(pty.spawn(sys.argv[1:])))", "/bin/sh", "-c", shim, "sh", command, ...args], {
    cwd,
    env: { ...process.env, ...env, TERM: "xterm-256color", COLUMNS: String(cols), LINES: String(rows) },
    stdio: ["pipe", "pipe", "pipe"],
  });
  let raw = "";
  const chunks = [];
  const record = (chunk) => {
    const text = chunk.toString("utf8");
    raw += text;
    chunks.push(text);
  };
  child.stdout.on("data", record);
  child.stderr.on("data", record);

  const screen = () => {
    const parts = raw.split(CLEAR_SCREEN);
    return stripAnsi(parts.at(-1) ?? raw);
  };
  const fullLog = () => stripAnsi(raw);
  const send = (text) => child.stdin.write(text);
  const key = (name) => {
    const value = KEYS[name];
    if (!value) throw new Error(`unknown key: ${name}`);
    send(value);
  };
  const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });
  const waitFor = async (pattern, { timeoutMs = 15000, useFullLog = false } = {}) => {
    const deadline = Date.now() + timeoutMs;
    const matches = pattern instanceof RegExp
      ? (value) => pattern.test(value)
      : (value) => value.includes(String(pattern));
    while (Date.now() < deadline) {
      const value = useFullLog ? fullLog() : screen();
      if (matches(value)) return value;
      await sleep(100);
    }
    throw new Error(`waitFor timed out after ${timeoutMs}ms looking for: ${pattern}\n--- screen ---\n${screen()}`);
  };
  const close = async () => {
    if (child.exitCode === null) child.kill("SIGTERM");
    await new Promise((resolve) => {
      if (child.exitCode !== null) resolve();
      else child.once("exit", resolve);
    });
    return { code: child.exitCode, fullLog: fullLog() };
  };
  return { child, send, key, waitFor, screen, fullLog, chunks, close, sleep };
}
