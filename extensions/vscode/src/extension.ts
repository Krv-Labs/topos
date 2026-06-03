import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import { execFile } from 'child_process';
import { promisify } from 'util';
import {
    getPlatformKey,
    resolveHomePath,
    computeFileSha256,
    fetchJson,
    downloadFile,
    selectBinaryFromManifest,
    isMcpApiAvailable,
    buildServerInvocation,
    buildCliInvocation,
    resolveEvaluateLanguages,
    TOPOS_SUPPORTED_LANGUAGES,
} from './runtime';

const execFileAsync = promisify(execFile);

// User-facing product name (Marketplace title). The agent-facing MCP server label
// is kept short (see MCP_SERVER_LABEL) so tool listings stay clean.
const OUTPUT_CHANNEL_NAME = "Topos";
const MCP_SERVER_LABEL = "Topos";

// The official, static release manifest URL where we publish compatible binaries and checksums.
const MANIFEST_URL = "https://raw.githubusercontent.com/Krv-Labs/topos/main/releases.json";
const BUNDLED_BINARY_RELATIVE_PATH = path.join('bin', 'topos');

const GITNEXUS_INSTALL_COMMAND = "npm install -g gitnexus";
const GITNEXUS_REPO_URL = "https://github.com/abhigyanpatwari/GitNexus";
const GITNEXUS_PROMPT_DISMISSED_KEY = "gitnexusPromptDismissed";

export async function activate(context: vscode.ExtensionContext) {
    const outputChannel = vscode.window.createOutputChannel(OUTPUT_CHANNEL_NAME);
    context.subscriptions.push(outputChannel);
    outputChannel.appendLine("Topos extension activating...");
    outputChannel.appendLine(`Host: ${vscode.env.appName} ${vscode.version}`);

    // 1. Guard native Windows environments (binaries are macOS/Linux/WSL only)
    if (process.platform === 'win32') {
        outputChannel.appendLine("Native Windows detected. Showing block warning.");
        vscode.window.showWarningMessage(
            "Topos does not currently support native Windows. Please open your workspace inside WSL (Windows Subsystem for Linux), or install the CLI manually via 'pip install topos'.",
            "Open WSL Guide"
        ).then(selection => {
            if (selection === "Open WSL Guide") {
                vscode.env.openExternal(vscode.Uri.parse("https://code.visualstudio.com/docs/remote/wsl"));
            }
        });
        return;
    }

    // 2. Feature-detect the MCP API before using it. Hosts that track an older VS Code
    //    base (e.g. some Cursor builds) may not expose vscode.lm / McpStdioServerDefinition.
    //    Fail safe with an actionable message instead of throwing during activation.
    if (!isMcpApiAvailable(vscode)) {
        const hasLm = typeof (vscode as { lm?: unknown }).lm !== 'undefined';
        const hasStdioDef = typeof (vscode as { McpStdioServerDefinition?: unknown }).McpStdioServerDefinition === 'function';
        outputChannel.appendLine(
            `ERROR: This host does not expose the MCP server definition API (vscode.lm: ${hasLm}, McpStdioServerDefinition: ${hasStdioDef}).`
        );
        vscode.window.showWarningMessage(
            "Topos requires an MCP-capable host (VS Code 1.120 or newer, or a compatible editor). The Topos MCP server was not registered.",
            "View Documentation"
        ).then(selection => {
            if (selection === "View Documentation") {
                vscode.env.openExternal(vscode.Uri.parse("https://docs.krv.ai/topos"));
            }
        });
        return;
    }

    const providerId = 'topos-mcp';
    const didChangeEmitter = new vscode.EventEmitter<void>();
    context.subscriptions.push(didChangeEmitter);

    const provider: vscode.McpServerDefinitionProvider<vscode.McpStdioServerDefinition> = {
        onDidChangeMcpServerDefinitions: didChangeEmitter.event,
        provideMcpServerDefinitions: (_token: vscode.CancellationToken) => {
            const env = getWorkspaceEnv();
            return [
                new vscode.McpStdioServerDefinition(
                    MCP_SERVER_LABEL,
                    "topos",
                    ["mcp"],
                    env,
                    context.extension.packageJSON.version
                )
            ];
        },
        resolveMcpServerDefinition: async (server: vscode.McpStdioServerDefinition, token: vscode.CancellationToken) => {
            outputChannel.appendLine("Resolving Topos executable path...");
            const resolvedBinaryPath = await resolveToposExecutable(context, outputChannel, token);

            if (!resolvedBinaryPath) {
                outputChannel.appendLine("ERROR: Topos executable could not be resolved.");
                vscode.window.showErrorMessage(
                    "Topos could not start because no bundled, cached, local, or downloadable Topos runtime was available.",
                    "View Documentation"
                ).then(selection => {
                    if (selection === "View Documentation") {
                        vscode.env.openExternal(vscode.Uri.parse("https://docs.krv.ai/topos"));
                    }
                });
                return undefined;
            }

            const { command, args } = buildServerInvocation(resolvedBinaryPath);
            server.command = command;
            server.args = args;
            server.env = getWorkspaceEnv();
            server.version = context.extension.packageJSON.version;

            outputChannel.appendLine(`Resolved MCP server command: ${command} ${args.join(" ")}`);
            return server;
        }
    };

    context.subscriptions.push(vscode.lm.registerMcpServerDefinitionProvider(providerId, provider));
    outputChannel.appendLine("Topos MCP Server Provider registered successfully.");

    // 3. Command Palette workflows (dependency graph + project evaluation).
    context.subscriptions.push(
        vscode.commands.registerCommand('topos.generateDependencyGraph', () => generateDependencyGraph(context, outputChannel)),
        vscode.commands.registerCommand('topos.evaluateProject', () => evaluateProject(context, outputChannel))
    );

    // 4. COMPOSABLE requires GitNexus. Detect it; if absent, surface a non-blocking,
    //    dismissible prompt. SIMPLE and SECURE keep working regardless.
    void checkGitNexusAvailability(context, outputChannel);
}

export function deactivate() {}

function getWorkspaceEnv(): Record<string, string | number | null> {
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    return workspaceRoot ? { TOPOS_MCP_FILE_ROOT: workspaceRoot } : {};
}

/**
 * Executes the fallback activation hierarchy to find or download the Topos binary.
 */
async function resolveToposExecutable(context: vscode.ExtensionContext, output: vscode.OutputChannel, token: vscode.CancellationToken): Promise<string | undefined> {
    const config = vscode.workspace.getConfiguration('topos');
    const executablePathOverride = config.get<string>('executablePath');
    const autoDiscover = config.get<boolean>('autoDiscover', true);
    const autoDownload = config.get<boolean>('autoDownload', true);

    // Step 1: User custom executable path override
    if (executablePathOverride && executablePathOverride.trim() !== "") {
        const expandedPath = resolveHomePath(executablePathOverride);
        if (fs.existsSync(expandedPath)) {
            if (await testToposExecutable(expandedPath)) {
                output.appendLine(`Step 1: Using custom executable path override: ${expandedPath}`);
                return expandedPath;
            }
            output.appendLine(`Step 1: Custom path exists but does not execute 'topos --version': ${expandedPath}`);
        } else {
            output.appendLine(`Step 1: Custom path configured but does not exist: ${expandedPath}`);
        }
    }

    // Step 2: Bundled platform runtime shipped inside the target VSIX
    output.appendLine("Step 2: Checking bundled Topos runtime...");
    const bundledBinaryPath = context.asAbsolutePath(BUNDLED_BINARY_RELATIVE_PATH);
    if (fs.existsSync(bundledBinaryPath)) {
        await ensureExecutable(bundledBinaryPath, output);
        if (await testToposExecutable(bundledBinaryPath)) {
            output.appendLine(`Using bundled Topos runtime: ${bundledBinaryPath}`);
            return bundledBinaryPath;
        }
        output.appendLine(`Bundled Topos runtime exists but failed its version check: ${bundledBinaryPath}`);
    } else {
        output.appendLine("No bundled Topos runtime found in this extension package.");
    }

    // Step 3: Standalone locally-cached binary check (via Global Storage)
    output.appendLine("Step 3: Checking standalone cached binary folder...");
    const storagePath = context.globalStorageUri.fsPath;
    const cachedBinaryPath = path.join(storagePath, 'topos-cli');

    // Check if we have cached metadata about a previously verified download
    const cachedVersion = context.globalState.get<string>('cachedVersion');
    const cachedSha256 = context.globalState.get<string>('cachedSha256');

    if (fs.existsSync(cachedBinaryPath)) {
        if (cachedVersion && cachedSha256 && await cachedBinaryMatches(cachedBinaryPath, cachedSha256, output)) {
            await ensureExecutable(cachedBinaryPath, output);
            if (await testToposExecutable(cachedBinaryPath)) {
                output.appendLine(`Found cached standalone binary of version ${cachedVersion} with validated SHA-256.`);
                return cachedBinaryPath;
            }
            output.appendLine("Cached binary hash is valid but executable check failed. Removing cached binary.");
            await fs.promises.unlink(cachedBinaryPath).catch(() => {});
        } else {
            output.appendLine("Cached binary exists, but metadata is missing or invalid. Will continue fallback resolution.");
        }
    }

    // Step 4: Global PATH check
    output.appendLine("Step 4: Checking system PATH for 'topos' CLI...");
    try {
        await execFileAsync('topos', ['--version']);
        output.appendLine("Found 'topos' executable globally on system PATH.");
        return 'topos';
    } catch {
        output.appendLine("'topos' executable is not available globally on system PATH.");
    }

    // Step 5: Auto-discover Topos inside active Python virtual environment
    if (autoDiscover) {
        output.appendLine("Step 5: Checking active Python virtual environment...");
        const pythonInterpreter = await getPythonInterpreterPath();
        if (token.isCancellationRequested) return undefined;
        if (pythonInterpreter) {
            output.appendLine(`Found active Python interpreter: ${pythonInterpreter}`);
            const runsTopos = await testPythonTopos(pythonInterpreter);
            if (runsTopos) {
                output.appendLine("Active virtual environment successfully executes 'topos' CLI. Using venv interpreter.");
                return pythonInterpreter;
            } else {
                output.appendLine("Topos is not installed in the active virtual environment.");
            }
        } else {
            output.appendLine("No active virtual environment detected via Python Extension API.");
        }
    }

    // Step 6: Remote manifest fetch & background standalone download
    if (autoDownload) {
        output.appendLine("Step 6: Fetching remote manifest to resolve and download the latest compatible binary...");
        try {
            await fs.promises.mkdir(storagePath, { recursive: true });
            const manifest = await fetchJson(MANIFEST_URL);
            if (token.isCancellationRequested) return undefined;

            const platformKey = getPlatformKey();
            if (!platformKey) {
                output.appendLine(`Unrecognized execution platform: ${process.platform}-${process.arch}. Standalone binaries unavailable.`);
                return undefined;
            }

            const binaryInfo = selectBinaryFromManifest(manifest, platformKey);
            if (!binaryInfo) {
                output.appendLine(`No standalone binary listed in manifest for platform: ${platformKey}`);
                return undefined;
            }

            const latestVersion = manifest.latest_cli_version;
            const downloadUrl = binaryInfo.url;
            const expectedSha256 = binaryInfo.sha256;

            output.appendLine(`Latest compatible Topos CLI version resolved: ${latestVersion}`);
            output.appendLine(`Download URL: ${downloadUrl}`);
            output.appendLine(`Expected SHA-256: ${expectedSha256}`);

            // Double check if the existing local binary matches the manifest checksum (ignoring cached state metadata)
            if (fs.existsSync(cachedBinaryPath)) {
                output.appendLine("Calculating SHA-256 of local cached binary...");
                const localHash = await computeFileSha256(cachedBinaryPath);
                if (localHash === expectedSha256) {
                    output.appendLine("Local cached binary SHA-256 matches expected checksum exactly. Updating cached state metadata.");
                    await ensureExecutable(cachedBinaryPath, output);
                    if (!await testToposExecutable(cachedBinaryPath)) {
                        output.appendLine("Local cached binary hash matched manifest but executable check failed. Removing cached binary.");
                        await fs.promises.unlink(cachedBinaryPath).catch(() => {});
                    } else {
                        await context.globalState.update('cachedVersion', latestVersion);
                        await context.globalState.update('cachedSha256', expectedSha256);
                        return cachedBinaryPath;
                    }
                } else {
                    output.appendLine("Local file exists but SHA-256 does not match. Proceeding to fresh download.");
                    await fs.promises.unlink(cachedBinaryPath).catch(() => {});
                }
            }

            // Fresh download under a progress notification bar
            const success = await vscode.window.withProgress({
                location: vscode.ProgressLocation.Notification,
                title: `Downloading Topos CLI (v${latestVersion})...`,
                cancellable: false
            }, async (progress) => {
                let lastPercent = 0;
                await downloadFile(downloadUrl, cachedBinaryPath, (fraction) => {
                    const percent = Math.round(fraction * 100);
                    if (percent > lastPercent) {
                        progress.report({ increment: percent - lastPercent, message: `${percent}%` });
                        lastPercent = percent;
                    }
                });
                return true;
            });

            if (success) {
                output.appendLine("Download complete. Calculating SHA-256 hash...");
                const downloadedHash = await computeFileSha256(cachedBinaryPath);

                if (downloadedHash !== expectedSha256) {
                    output.appendLine(`ERROR: Cryptographic hash verification failed!`);
                    output.appendLine(`Calculated: ${downloadedHash}`);
                    output.appendLine(`Expected:   ${expectedSha256}`);
                    await fs.promises.unlink(cachedBinaryPath).catch(() => {});
                    return undefined;
                }

                output.appendLine("SHA-256 validation succeeded. Applying executable permissions via async chmod...");
                await fs.promises.chmod(cachedBinaryPath, 0o755);

                if (!await testToposExecutable(cachedBinaryPath)) {
                    output.appendLine("Downloaded binary passed SHA-256 validation but failed executable check. Removing cached binary.");
                    await fs.promises.unlink(cachedBinaryPath).catch(() => {});
                    return undefined;
                }

                output.appendLine("Updating extension cache metadata.");
                await context.globalState.update('cachedVersion', latestVersion);
                await context.globalState.update('cachedSha256', expectedSha256);

                return cachedBinaryPath;
            }
        } catch (err: any) {
            output.appendLine(`ERROR: Remote resolution / download failed: ${err?.message || err}`);
        }
    }

    return undefined;
}

async function ensureExecutable(filePath: string, output: vscode.OutputChannel): Promise<void> {
    if (process.platform === 'win32') return;

    try {
        await fs.promises.chmod(filePath, 0o755);
    } catch (err: any) {
        output.appendLine(`WARNING: Could not apply executable permissions to ${filePath}: ${err?.message || err}`);
    }
}

async function cachedBinaryMatches(filePath: string, expectedSha256: string, output: vscode.OutputChannel): Promise<boolean> {
    output.appendLine("Calculating SHA-256 of cached binary...");
    const localHash = await computeFileSha256(filePath);
    if (localHash === expectedSha256) return true;

    output.appendLine("Cached binary SHA-256 does not match stored metadata. Removing cached binary.");
    await fs.promises.unlink(filePath).catch(() => {});
    return false;
}

/**
 * Resolves the active Python interpreter path using the official Python Extension API.
 */
async function getPythonInterpreterPath(): Promise<string | undefined> {
    const pythonExtension = vscode.extensions.getExtension('ms-python.python');
    if (!pythonExtension) return undefined;

    if (!pythonExtension.isActive) {
        await pythonExtension.activate();
    }

    const pythonAPI = pythonExtension.exports;
    const workspaceFolder = vscode.workspace.workspaceFolders?.[0];
    const activeEnvPath = pythonAPI.environments.getActiveEnvironmentPath(workspaceFolder?.uri);
    const resolvedEnv = await pythonAPI.environments.resolveEnvironment(activeEnvPath);

    return resolvedEnv?.executable?.uri?.fsPath;
}

/**
 * Checks if the specified Python executable is able to execute the Topos CLI successfully.
 */
async function testPythonTopos(pythonPath: string): Promise<boolean> {
    try {
        await execFileAsync(pythonPath, ['-m', 'topos.cli', '--version']);
        return true;
    } catch {
        return false;
    }
}

async function testToposExecutable(executablePath: string): Promise<boolean> {
    try {
        await execFileAsync(executablePath, ['--version']);
        return true;
    } catch {
        return false;
    }
}

/**
 * Detects whether the GitNexus CLI is available on PATH. COMPOSABLE scoring
 * depends on it; SIMPLE and SECURE do not.
 */
async function isGitNexusOnPath(): Promise<boolean> {
    try {
        await execFileAsync('gitnexus', ['--version']);
        return true;
    } catch {
        return false;
    }
}

/**
 * Non-blocking GitNexus availability check. Logs status and, when absent, shows a
 * dismissible prompt offering a guided install. Never blocks activation.
 */
async function checkGitNexusAvailability(context: vscode.ExtensionContext, output: vscode.OutputChannel): Promise<void> {
    if (await isGitNexusOnPath()) {
        output.appendLine("GitNexus detected on PATH. The COMPOSABLE pillar is available.");
        return;
    }

    output.appendLine(
        "GitNexus was not found on PATH. SIMPLE and SECURE work normally, but the COMPOSABLE pillar " +
        `is unavailable until GitNexus is installed (${GITNEXUS_INSTALL_COMMAND}) and a dependency graph is generated.`
    );

    if (context.globalState.get<boolean>(GITNEXUS_PROMPT_DISMISSED_KEY)) {
        return;
    }

    const selection = await vscode.window.showInformationMessage(
        "Topos: install GitNexus to enable the COMPOSABLE code-quality pillar. SIMPLE and SECURE work without it.",
        "Install GitNexus",
        "Learn More",
        "Don't Show Again"
    );

    if (selection === "Install GitNexus") {
        installGitNexus(output);
    } else if (selection === "Learn More") {
        vscode.env.openExternal(vscode.Uri.parse(GITNEXUS_REPO_URL));
    } else if (selection === "Don't Show Again") {
        await context.globalState.update(GITNEXUS_PROMPT_DISMISSED_KEY, true);
    }
}

/**
 * Launches the GitNexus install in a visible terminal so the user can review output.
 */
function installGitNexus(output: vscode.OutputChannel): void {
    output.appendLine(`Launching GitNexus install: ${GITNEXUS_INSTALL_COMMAND}`);
    const terminal = vscode.window.createTerminal("Install GitNexus");
    terminal.show();
    terminal.sendText(GITNEXUS_INSTALL_COMMAND);
}

/**
 * Command handler: generates the dependency graph by running `topos depgraph generate`
 * in a terminal at the workspace root. This produces the `.gitnexus/` store the
 * COMPOSABLE pillar reads from.
 */
async function generateDependencyGraph(context: vscode.ExtensionContext, output: vscode.OutputChannel): Promise<void> {
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!workspaceRoot) {
        vscode.window.showErrorMessage("Topos: open a workspace folder before generating a dependency graph.");
        return;
    }

    if (!await isGitNexusOnPath()) {
        const selection = await vscode.window.showWarningMessage(
            "Topos: GitNexus is required to generate a dependency graph but was not found on PATH.",
            "Install GitNexus",
            "Learn More"
        );
        if (selection === "Install GitNexus") {
            installGitNexus(output);
        } else if (selection === "Learn More") {
            vscode.env.openExternal(vscode.Uri.parse(GITNEXUS_REPO_URL));
        }
        return;
    }

    await runToposCliInTerminal(context, output, {
        terminalName: "Topos: Generate Dependency Graph",
        cwd: workspaceRoot,
        cliArgs: ["depgraph", "generate"],
        failureMessage: "Topos: could not resolve the Topos executable to generate a dependency graph.",
    });
}

/**
 * Command handler: runs `topos evaluate <path> -r` with verbose per-file output in a
 * terminal. Defaults to `src/` when present, otherwise the workspace root.
 */
async function evaluateProject(context: vscode.ExtensionContext, output: vscode.OutputChannel): Promise<void> {
    const workspaceRoot = vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
    if (!workspaceRoot) {
        vscode.window.showErrorMessage("Topos: open a workspace folder before evaluating the project.");
        return;
    }

    const config = vscode.workspace.getConfiguration("topos");
    const evaluatePath = pickEvaluatePath(workspaceRoot, config.get<string>("evaluatePath", ""));
    const scanRoot = path.isAbsolute(evaluatePath)
        ? evaluatePath
        : path.join(workspaceRoot, evaluatePath);
    const configuredLanguage = config.get<string>("evaluateLanguage", "auto");
    const preferences = config.get<string>("evaluatePreferences", "").trim();
    const verbose = config.get<boolean>("evaluateVerbose", true);

    output.appendLine(`Scanning for source languages under ${evaluatePath}...`);
    const languages = resolveEvaluateLanguages(scanRoot, configuredLanguage);
    if (languages.length === 0) {
        const hint = configuredLanguage.trim().toLowerCase() === "auto"
            ? `No supported source files found under ${evaluatePath}. Topos supports: ${TOPOS_SUPPORTED_LANGUAGES.join(", ")}.`
            : `Invalid or unsupported topos.evaluateLanguage "${configuredLanguage}". Use auto or one of: ${TOPOS_SUPPORTED_LANGUAGES.join(", ")}.`;
        vscode.window.showErrorMessage(`Topos: ${hint}`);
        output.appendLine(hint);
        return;
    }

    output.appendLine(`Detected languages: ${languages.join(", ")}`);

    const hasGitnexus = fs.existsSync(path.join(workspaceRoot, ".gitnexus"));
    if (!hasGitnexus) {
        output.appendLine(
            "No .gitnexus/ directory in workspace. SIMPLE and SECURE will run; " +
            "COMPOSABLE needs **Topos: Generate Dependency Graph** first."
        );
    }

    const cliArgSequences = languages.map((language) =>
        buildEvaluateCliArgs(evaluatePath, language, { verbose, preferences, hasGitnexus })
    );

    await runToposCliInTerminal(context, output, {
        terminalName: "Topos: Evaluate Project",
        cwd: workspaceRoot,
        cliArgs: cliArgSequences[0],
        cliArgSequences: cliArgSequences.length > 1 ? cliArgSequences.slice(1) : undefined,
        preambleLines: languages.length > 1
            ? [`echo "Topos: evaluating ${languages.join(", ")} under ${evaluatePath}"`]
            : undefined,
        failureMessage: "Topos: could not resolve the Topos executable to evaluate the project.",
    });
}

function buildEvaluateCliArgs(
    evaluatePath: string,
    language: string,
    options: { verbose: boolean; preferences: string; hasGitnexus: boolean }
): string[] {
    const cliArgs = ["evaluate", evaluatePath, "-r", "--language", language];
    if (options.verbose) {
        cliArgs.push("-v");
    }
    if (options.preferences) {
        cliArgs.push("--preferences", options.preferences);
    }
    if (options.hasGitnexus) {
        cliArgs.push("--gitnexus-dir", ".gitnexus");
    }
    return cliArgs;
}

/**
 * Chooses the directory passed to `topos evaluate`: explicit setting, else `src/` if it
 * exists, else `.` (workspace root).
 */
function pickEvaluatePath(workspaceRoot: string, configuredPath: string): string {
    const trimmed = configuredPath.trim();
    if (trimmed) {
        return trimmed;
    }
    const srcDir = path.join(workspaceRoot, "src");
    if (fs.existsSync(srcDir) && fs.statSync(srcDir).isDirectory()) {
        return "src";
    }
    return ".";
}

async function runToposCliInTerminal(
    context: vscode.ExtensionContext,
    output: vscode.OutputChannel,
    options: {
        terminalName: string;
        cwd: string;
        cliArgs: string[];
        /** Additional evaluate (or other) invocations run in the same terminal after the first. */
        cliArgSequences?: string[][];
        preambleLines?: string[];
        failureMessage: string;
    }
): Promise<void> {
    const source = new vscode.CancellationTokenSource();
    try {
        const resolvedBinaryPath = await resolveToposExecutable(context, output, source.token);
        if (!resolvedBinaryPath) {
            vscode.window.showErrorMessage(options.failureMessage);
            return;
        }

        const sequences = [options.cliArgs, ...(options.cliArgSequences ?? [])];
        const terminal = vscode.window.createTerminal({ name: options.terminalName, cwd: options.cwd });
        terminal.show();

        const shellLines: string[] = [...(options.preambleLines ?? [])];
        for (let i = 0; i < sequences.length; i += 1) {
            const { command, args } = buildCliInvocation(resolvedBinaryPath, sequences[i]);
            output.appendLine(`Running: ${command} ${args.join(" ")}`);
            if (sequences.length > 1) {
                const langFlag = sequences[i].indexOf("--language");
                const lang = langFlag >= 0 ? sequences[i][langFlag + 1] : "";
                shellLines.push(`echo ""`, `echo "=== Topos evaluate (${lang}) ==="`);
            }
            const quotedArgs = args.map(quoteArg).join(" ");
            shellLines.push(`${quoteArg(command)} ${quotedArgs}`);
        }

        terminal.sendText(shellLines.join("\n"));
    } finally {
        source.dispose();
    }
}

/**
 * Minimal POSIX shell quoting for terminal command construction.
 */
function quoteArg(arg: string): string {
    if (/^[A-Za-z0-9_\-./]+$/.test(arg)) {
        return arg;
    }
    return `'${arg.replace(/'/g, `'\\''`)}'`;
}
