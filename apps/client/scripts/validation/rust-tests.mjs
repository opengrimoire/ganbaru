import { spawn } from "node:child_process";
import { existsSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { availableParallelism } from "node:os";
import { dirname, join, relative } from "node:path";
import { createInterface } from "node:readline";

/**
 * @typedef {object} TestBinary
 * @property {string} executable Absolute path of the libtest binary.
 * @property {string} packageDir Package root, the working directory Cargo gives test binaries.
 * @property {string} label Target name and kind shown in the output.
 * @property {number} size Executable size, a cheap proxy for run time when ordering binaries.
 */

/**
 * @typedef {object} RustTestLimits
 * @property {number} cargoJobs Parallel Cargo build jobs.
 * @property {number} threads Runtime threads per test binary.
 * @property {number} lanes Test binaries allowed to run at the same time.
 */

/**
 * @typedef {object} CargoArtifact
 * @property {string} reason
 * @property {string} manifest_path
 * @property {{ name: string, kind: string[] }} target
 * @property {{ test: boolean }} profile
 * @property {string | null} executable
 */

/**
 * Narrow one line of Cargo's JSON output to a test executable artifact.
 * @param {unknown} message
 * @returns {message is CargoArtifact & { executable: string }}
 */
function isTestExecutable(message) {
  if (typeof message !== "object" || message === null) return false;
  const artifact = /** @type {Partial<CargoArtifact>} */ (message);
  return (
    artifact.reason === "compiler-artifact" &&
    typeof artifact.executable === "string" &&
    artifact.profile?.test === true &&
    typeof artifact.manifest_path === "string" &&
    typeof artifact.target?.name === "string" &&
    Array.isArray(artifact.target?.kind)
  );
}

/**
 * Build every workspace test binary without running it and return the executables Cargo reports.
 * Diagnostics are rendered to the inherited stderr; stdout carries only JSON messages.
 * @param {string} repoRoot
 * @param {number} cargoJobs
 * @returns {Promise<TestBinary[] | undefined>} The binaries, or undefined when the build failed.
 */
function buildTestBinaries(repoRoot, cargoJobs) {
  const args = ["test", "--workspace", "-j", String(cargoJobs), "--no-run", "--message-format=json-render-diagnostics"];
  return new Promise((resolvePromise, rejectPromise) => {
    const child = spawn("cargo", args, { cwd: repoRoot, stdio: ["ignore", "pipe", "inherit"] });
    /** @type {TestBinary[]} */
    const binaries = [];
    createInterface({ input: child.stdout }).on("line", (line) => {
      if (!line.startsWith("{")) return;
      const message = JSON.parse(line);
      if (!isTestExecutable(message)) return;
      binaries.push({
        executable: message.executable,
        packageDir: dirname(message.manifest_path),
        label: `${message.target.name} (${message.target.kind.join(", ")})`,
        size: statSync(message.executable).size,
      });
    });
    child.on("error", (error) => rejectPromise(new Error(`Could not start cargo: ${error.message}`)));
    child.on("close", (code) => resolvePromise(code === 0 ? binaries : undefined));
  });
}

/**
 * Run one test binary the way `cargo test` does, from its package root, buffering its output so
 * concurrent binaries print whole blocks.
 * @param {string} repoRoot
 * @param {TestBinary} binary
 * @param {number} threads
 * @returns {Promise<boolean>} Whether every test passed.
 */
function runTestBinary(repoRoot, binary, threads) {
  const started = performance.now();
  return new Promise((resolvePromise) => {
    const child = spawn(binary.executable, [`--test-threads=${threads}`], {
      cwd: binary.packageDir,
      env: { ...process.env, CARGO_MANIFEST_DIR: binary.packageDir },
      stdio: ["ignore", "pipe", "pipe"],
    });
    /** @type {Buffer[]} */
    const chunks = [];
    child.stdout.on("data", (chunk) => chunks.push(chunk));
    child.stderr.on("data", (chunk) => chunks.push(chunk));
    let settled = false;
    const report = (/** @type {boolean} */ passed, /** @type {string} */ note) => {
      if (settled) return;
      settled = true;
      const seconds = ((performance.now() - started) / 1000).toFixed(1);
      process.stdout.write(`\n     Running ${binary.label} ${relative(repoRoot, binary.executable)} (${seconds}s)\n`);
      process.stdout.write(Buffer.concat(chunks));
      if (note) process.stdout.write(`${note}\n`);
      resolvePromise(passed);
    };
    child.on("error", (error) => report(false, `error: could not start ${binary.executable}: ${error.message}`));
    child.on("close", (code, signal) => report(code === 0, code === 0 ? "" : `error: ${binary.label} failed (${signal ?? `exit code ${code}`})`));
  });
}

/** Last passing duration of each binary, kept beside the test executables so a target reset clears it. */
const DURATIONS_FILE = "rust-test-durations.json";

/**
 * Stable identity of a test binary across rebuilds, unlike its hashed executable name.
 * @param {TestBinary} binary
 */
function binaryKey(binary) {
  return `${binary.packageDir}#${binary.label}`;
}

/**
 * Read recorded durations, treating a missing or malformed file as no history.
 * @param {string} path
 * @returns {Record<string, number>}
 */
function readDurations(path) {
  if (!existsSync(path)) return {};
  try {
    const parsed = JSON.parse(readFileSync(path, "utf8"));
    if (typeof parsed !== "object" || parsed === null || Array.isArray(parsed)) return {};
    return Object.fromEntries(Object.entries(parsed).filter((entry) => typeof entry[1] === "number" && Number.isFinite(entry[1])));
  } catch (error) {
    process.stderr.write(`warning: ignoring unreadable ${path}: ${error instanceof Error ? error.message : String(error)}\n`);
    return {};
  }
}

/**
 * Record durations for the next run; a failed write only costs the ordering hint.
 * @param {string} path
 * @param {Record<string, number>} durations
 */
function writeDurations(path, durations) {
  try {
    writeFileSync(path, `${JSON.stringify(durations, null, 2)}\n`);
  } catch (error) {
    process.stderr.write(`warning: could not record test durations in ${path}: ${error instanceof Error ? error.message : String(error)}\n`);
  }
}

/**
 * Longest first, so the slowest binary never starts last: by recorded duration when every binary
 * has one, otherwise by executable size, a rough proxy on a fresh target or after new targets.
 * @param {TestBinary[]} binaries
 * @param {Record<string, number>} durations
 * @returns {TestBinary[]}
 */
export function orderBinaries(binaries, durations) {
  const known = binaries.every((binary) => binaryKey(binary) in durations);
  const weight = (/** @type {TestBinary} */ binary) => (known ? durations[binaryKey(binary)] : binary.size);
  return [...binaries].sort((left, right) => weight(right) - weight(left));
}

/**
 * Lanes allowed on this machine: never more test threads than logical cores, so small runners
 * fall back to one binary at a time.
 * @param {RustTestLimits} limits
 * @param {number} cores
 */
export function effectiveLanes(limits, cores) {
  return Math.max(1, Math.min(limits.lanes, Math.floor(cores / limits.threads)));
}

/**
 * Build and run every workspace test binary. `cargo test` runs binaries one after another, so the
 * longest binary alone sets most of the wall time; lanes run several at once, largest first, with
 * buffered output. A failing binary stops new binaries from starting, like `cargo test`; the ones
 * already running finish and report.
 * @param {string} repoRoot
 * @param {RustTestLimits} limits
 * @returns {Promise<boolean>} Whether the build and every binary passed.
 */
export async function runRustTests(repoRoot, limits) {
  const binaries = await buildTestBinaries(repoRoot, limits.cargoJobs);
  if (!binaries) return false;
  const durationsPath = binaries.length > 0 ? join(dirname(dirname(binaries[0].executable)), DURATIONS_FILE) : undefined;
  const previous = durationsPath ? readDurations(durationsPath) : {};
  const queue = orderBinaries(binaries, previous);
  /** @type {Record<string, number>} */
  const measured = { ...previous };
  let passed = true;
  const lane = async () => {
    for (let binary = queue.shift(); binary && passed; binary = queue.shift()) {
      const started = performance.now();
      if (await runTestBinary(repoRoot, binary, limits.threads)) measured[binaryKey(binary)] = performance.now() - started;
      else passed = false;
    }
  };
  const lanes = effectiveLanes(limits, availableParallelism());
  await Promise.all(Array.from({ length: lanes }, lane));
  if (durationsPath) writeDurations(durationsPath, measured);
  if (passed) process.stdout.write(`\nRan ${binaries.length} test binaries in ${lanes} lanes of ${limits.threads} threads.\n`);
  return passed;
}
