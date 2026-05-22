import * as vscode from 'vscode';
import * as fs from 'fs';
import * as path from 'path';
import * as crypto from 'crypto';
import * as https from 'https';
import * as url from 'url';
import { execFile } from 'child_process';
import { promisify } from 'util';

const execFileAsync = promisify(execFile);

// The official, static release manifest URL where we publish compatible binaries and checksums.
const MANIFEST_URL = "https://raw.githubusercontent.com/Krv-Labs/topos/main/releases.json";
const BUNDLED_BINARY_RELATIVE_PATH = path.join('bin', 'topos');

export async function activate(context: vscode.ExtensionContext) {
    const outputChannel = vscode.window.createOutputChannel("Topos Code Quality");
    outputChannel.appendLine("Topos Code Quality extension activating...");

    // 1. Guard native Windows environments (binaries are macOS/Linux/WSL only)
    if (process.platform === 'win32') {
        outputChannel.appendLine("Native Windows detected. Showing block warning.");
        vscode.window.showWarningMessage(
            "Topos Code Quality does not currently support native Windows. Please open your workspace inside WSL (Windows Subsystem for Linux), or install the CLI manually via 'pip install topos'.",
            "Open WSL Guide"
        ).then(selection => {
            if (selection === "Open WSL Guide") {
                vscode.env.openExternal(vscode.Uri.parse("https://code.visualstudio.com/docs/remote/wsl"));
            }
        });
        return;
    }

    const providerId = 'topos-mcp';
    const didChangeEmitter = new vscode.EventEmitter<void>();

    const provider: vscode.McpServerDefinitionProvider<vscode.McpStdioServerDefinition> = {
        onDidChangeMcpServerDefinitions: didChangeEmitter.event,
        provideMcpServerDefinitions: (_token: vscode.CancellationToken) => {
            const env = getWorkspaceEnv();
            return [
                new vscode.McpStdioServerDefinition(
                    "Topos Code Quality",
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
                    "Topos Code Quality could not start because no bundled, cached, local, or downloadable Topos runtime was available.",
                    "View Documentation"
                ).then(selection => {
                    if (selection === "View Documentation") {
                        vscode.env.openExternal(vscode.Uri.parse("https://docs.krv.ai/topos"));
                    }
                });
                return undefined;
            }

            let command = resolvedBinaryPath;
            let args = ["mcp"];

            if (resolvedBinaryPath.endsWith("python") || resolvedBinaryPath.endsWith("python3")) {
                command = resolvedBinaryPath;
                args = ["-m", "topos.cli", "mcp"];
            }

            server.command = command;
            server.args = args;
            server.env = getWorkspaceEnv();
            server.version = context.extension.packageJSON.version;

            outputChannel.appendLine(`Resolved MCP server command: ${command} ${args.join(" ")}`);
            return server;
        }
    };

    context.subscriptions.push(vscode.lm.registerMcpServerDefinitionProvider(providerId, provider));
    outputChannel.appendLine("Topos MCP Server Provider registered successfully with VS Code.");
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

            const binaryInfo = manifest.binaries?.[platformKey];
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
                title: `Downloading Topos Code Quality CLI (v${latestVersion})...`,
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

/**
 * Resolves paths starting with tilde (~) to the user's home directory.
 */
function resolveHomePath(filePath: string): string {
    if (filePath.startsWith('~')) {
        const homeDir = process.env.HOME || process.env.USERPROFILE || "";
        return path.join(homeDir, filePath.slice(1));
    }
    return path.resolve(filePath);
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
 * Map Node.js platform and architecture fields to the remote manifest platform keys.
 */
function getPlatformKey(): string | undefined {
    const platform = process.platform;
    const arch = process.arch;

    if (platform === 'darwin') {
        return arch === 'arm64' ? 'darwin-arm64' : 'darwin-x64';
    } else if (platform === 'linux') {
        return arch === 'arm64' ? 'linux-arm64' : 'linux-x64';
    }
    return undefined;
}

/**
 * Fetches JSON payload from a remote URL, recursively following HTTP/HTTPS redirects.
 */
function fetchJson(targetUrl: string): Promise<any> {
    return new Promise((resolve, reject) => {
        https.get(targetUrl, (response) => {
            const statusCode = response.statusCode ?? 0;

            // Follow redirects recursively
            if (statusCode >= 300 && statusCode < 400 && response.headers.location) {
                const nextUrl = url.resolve(targetUrl, response.headers.location);
                fetchJson(nextUrl).then(resolve).catch(reject);
                return;
            }

            if (statusCode !== 200) {
                reject(new Error(`Failed to fetch JSON: status code ${statusCode}`));
                return;
            }

            let body = '';
            response.on('data', chunk => body += chunk);
            response.on('end', () => {
                try {
                    resolve(JSON.parse(body));
                } catch (e) {
                    reject(e);
                }
            });
        }).on('error', err => reject(err));
    });
}

/**
 * Downloads a file from a URL to a local destination path, recursively following redirects.
 */
function downloadFile(targetUrl: string, destPath: string, onProgress?: (fraction: number) => void): Promise<void> {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(destPath);

        https.get(targetUrl, (response) => {
            const statusCode = response.statusCode ?? 0;

            // Follow redirects recursively
            if (statusCode >= 300 && statusCode < 400 && response.headers.location) {
                file.close();
                fs.unlink(destPath, () => {});
                const nextUrl = url.resolve(targetUrl, response.headers.location);
                downloadFile(nextUrl, destPath, onProgress).then(resolve).catch(reject);
                return;
            }

            if (statusCode !== 200) {
                file.close();
                fs.unlink(destPath, () => {});
                reject(new Error(`Failed to download: status code ${statusCode}`));
                return;
            }

            const totalBytes = parseInt(response.headers['content-length'] ?? '0', 10);
            let downloadedBytes = 0;

            response.on('data', (chunk) => {
                downloadedBytes += chunk.length;
                if (totalBytes > 0 && onProgress) {
                    onProgress(downloadedBytes / totalBytes);
                }
            });

            response.pipe(file);

            file.on('finish', () => {
                file.close();
                resolve();
            });

            file.on('error', (err) => {
                file.close();
                fs.unlink(destPath, () => {});
                reject(err);
            });
        }).on('error', (err) => {
            file.close();
            fs.unlink(destPath, () => {});
            reject(err);
        });
    });
}

/**
 * Computes the SHA-256 hash of a local file.
 */
function computeFileSha256(filePath: string): Promise<string> {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        const stream = fs.createReadStream(filePath);

        stream.on('error', err => reject(err));
        stream.on('data', chunk => hash.update(chunk));
        stream.on('end', () => resolve(hash.digest('hex')));
    });
}
