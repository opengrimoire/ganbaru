#!/usr/bin/env node

import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = path.resolve(SCRIPT_DIR, "..", "..", "..", "..");
const WORKFLOW_DIR = ".github/workflows";

/** Secrets that sign or publish release artifacts. */
const SIGNING_SECRET_PATTERN = /^(?:ANDROID_KEYSTORE|ANDROID_KEY_|TAURI_SIGNING_PRIVATE_KEY|GANBARU_PACKAGE_REPO_GPG|AUR_SSH)/u;

/** Commands that install or execute workspace dependencies or build output. */
const FORBIDDEN_COMMANDS = [
  { label: "pnpm", pattern: /\bpnpm\b/u },
  { label: "npm", pattern: /\bnpm\b/u },
  { label: "npx", pattern: /\bnpx\b/u },
  { label: "yarn", pattern: /\byarn\b/u },
  { label: "corepack", pattern: /\bcorepack\b/u },
  { label: "cargo", pattern: /\bcargo\b/u },
  { label: "gradle", pattern: /\bgradlew?\b/u },
  { label: "vite", pattern: /\bvite\b/u },
  { label: "node_modules", pattern: /\bnode_modules\b/u },
];

/** Actions a signing job may use; none of them runs workspace code. */
const ALLOWED_SIGNING_ACTIONS = new Set([
  "actions/checkout",
  "actions/setup-node",
  "actions/download-artifact",
  "actions/upload-artifact",
]);

/** Fixture the scanner must reject, so a parser regression cannot pass silently. */
const REJECTED_FIXTURE = `jobs:
  sign:
    steps:
      - name: install
        run: |
          pnpm install --frozen-lockfile
      - name: sign
        env:
          KEY: \${{ secrets.TAURI_SIGNING_PRIVATE_KEY }}
        run: node sign.mjs
`;

/** Fixture with a download that is never checksum-verified, which the download check must reject. */
const UNVERIFIED_DOWNLOAD_FIXTURE = `jobs:
  build:
    steps:
      - name: tools
        run: |
          curl -fsSL https://example.com/tools.zip -o tools.zip
          unzip tools.zip
`;

/** Fixture that stores a token in a remote URL, which the credential URL check must reject. */
const CREDENTIAL_URL_FIXTURE = `jobs:
  publish:
    steps:
      - name: remote
        run: git remote set-url origin "https://x-access-token:\${GITHUB_TOKEN}@github.com/\${GITHUB_REPOSITORY}.git"
`;

/** Count leading spaces. */
function indentation(line) {
  return line.length - line.trimStart().length;
}

/**
 * Splits a workflow into its jobs using indentation, without a YAML dependency.
 *
 * @param {string} text Workflow source.
 * @returns {Map<string, string[]>} Job id to its non-comment lines.
 */
export function workflowJobs(text) {
  const jobs = new Map();
  let inJobs = false;
  let jobIndent = -1;
  let current = null;
  for (const line of text.split(/\r?\n/u)) {
    const trimmed = line.trim();
    if (!trimmed || trimmed.startsWith("#")) continue;
    const indent = indentation(line);
    if (indent === 0) {
      inJobs = trimmed === "jobs:";
      current = null;
      jobIndent = -1;
      continue;
    }
    if (!inJobs) continue;
    if (jobIndent < 0) jobIndent = indent;
    if (indent === jobIndent) {
      const header = /^([A-Za-z0-9_-]+):$/u.exec(trimmed);
      if (!header) throw new Error(`Unexpected line at job level: ${trimmed}`);
      current = [];
      jobs.set(header[1], current);
      continue;
    }
    if (indent < jobIndent || !current) throw new Error(`Unexpected indentation: ${trimmed}`);
    current.push(line);
  }
  return jobs;
}

/** Collects the shell text of every `run` key in a job. */
function runCommands(lines) {
  const commands = [];
  for (let index = 0; index < lines.length; index += 1) {
    const match = /^(\s*)(?:- )?run:\s*(.*)$/u.exec(lines[index]);
    if (!match) continue;
    const value = match[2];
    if (!/^[|>][+-]?$/u.test(value)) {
      commands.push(value);
      continue;
    }
    const keyIndent = match[1].length + (lines[index].trimStart().startsWith("- ") ? 2 : 0);
    while (index + 1 < lines.length && indentation(lines[index + 1]) > keyIndent) {
      index += 1;
      commands.push(lines[index].trim());
    }
  }
  return commands;
}

/**
 * Lists violations where a job that receives signing secrets also installs or runs workspace code.
 *
 * @param {string} text Workflow source.
 * @returns {string[]} Human readable violations.
 */
export function signingIsolationViolations(text) {
  const violations = [];
  for (const [job, lines] of workflowJobs(text)) {
    const body = lines.join("\n");
    const secrets = [...body.matchAll(/\bsecrets\.([A-Za-z0-9_]+)/gu)]
      .map((match) => match[1])
      .filter((name) => SIGNING_SECRET_PATTERN.test(name));
    if (!secrets.length) continue;
    for (const command of runCommands(lines)) {
      for (const { label, pattern } of FORBIDDEN_COMMANDS) {
        if (pattern.test(command)) {
          violations.push(`${job}: runs ${label} while holding ${secrets[0]}: ${command}`);
        }
      }
    }
    for (const line of lines) {
      const uses = /^\s*(?:- )?uses:\s*([^@\s]+)/u.exec(line);
      if (uses && !ALLOWED_SIGNING_ACTIONS.has(uses[1])) {
        violations.push(`${job}: uses ${uses[1]} while holding ${secrets[0]}`);
      }
    }
  }
  return violations;
}

/**
 * Lists downloads that are not checked against a pinned SHA-256 by the very next command.
 *
 * @param {string} text Workflow source.
 * @returns {string[]} Human readable violations.
 */
export function unverifiedDownloadViolations(text) {
  const violations = [];
  for (const [job, lines] of workflowJobs(text)) {
    const commands = runCommands(lines);
    commands.forEach((command, index) => {
      const downloads = /^(?:curl|wget)\b/u.test(command) && /\s(?:-o|-O|--output)\b/u.test(command);
      if (downloads && !/\bsha256sum -c\b/u.test(commands[index + 1] ?? "")) {
        violations.push(`${job}: download is not followed by sha256sum -c: ${command}`);
      }
    });
  }
  return violations;
}

/**
 * Lists commands that put credentials in an HTTP URL, where git persists them in `.git/config` for later steps.
 *
 * @param {string} text Workflow source.
 * @returns {string[]} Human readable violations.
 */
export function credentialUrlViolations(text) {
  const violations = [];
  for (const [job, lines] of workflowJobs(text)) {
    for (const command of runCommands(lines)) {
      if (/https?:\/\/[^\s"'/@]+@/u.test(command)) {
        violations.push(`${job}: URL carries credentials: ${command}`);
      }
    }
  }
  return violations;
}

if (!signingIsolationViolations(REJECTED_FIXTURE).length) {
  throw new Error("Signing isolation check accepted its rejected fixture");
}
if (!unverifiedDownloadViolations(UNVERIFIED_DOWNLOAD_FIXTURE).length) {
  throw new Error("Download check accepted its unverified fixture");
}
if (!credentialUrlViolations(CREDENTIAL_URL_FIXTURE).length) {
  throw new Error("Credential URL check accepted its rejected fixture");
}

const workflowPaths = readdirSync(path.join(REPO_ROOT, WORKFLOW_DIR))
  .filter((name) => /\.ya?ml$/u.test(name))
  .sort()
  .map((name) => `${WORKFLOW_DIR}/${name}`);
const workflows = workflowPaths.map((relativePath) => ({
  relativePath,
  text: readFileSync(path.join(REPO_ROOT, relativePath), "utf8"),
}));

/** Prints each violation under a heading and reports whether any were found. */
function reportViolations(heading, check) {
  const failures = workflows.flatMap(({ relativePath, text }) =>
    check(text).map((violation) => `${relativePath}: ${violation}`),
  );
  if (!failures.length) return false;
  console.error(heading);
  for (const failure of failures) console.error(`  ${failure}`);
  return true;
}

const signingFailed = reportViolations(
  "Signing secrets must stay in jobs that do not install or run workspace code:",
  signingIsolationViolations,
);
const downloadFailed = reportViolations(
  "Downloaded tools must be verified against a pinned SHA-256 before use:",
  unverifiedDownloadViolations,
);
const credentialFailed = reportViolations(
  "Workflow tokens must be passed per command, never stored in a remote URL:",
  credentialUrlViolations,
);
if (signingFailed || downloadFailed || credentialFailed) process.exit(1);
console.log(
  `${workflowPaths.length} workflows keep signing isolated, verify every download, and keep credentials out of URLs.`,
);
