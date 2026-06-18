import * as fs from 'fs';
import * as path from 'path';
import * as crypto from 'crypto';
import * as http from 'http';
import * as https from 'https';
import * as url from 'url';

export const REQUEST_TIMEOUT_MS = 15000;
export const MAX_REDIRECTS = 5;

export interface ManifestBinary {
    url: string;
    sha256: string;
}

export interface ReleaseManifest {
    latest_cli_version?: string;
    binaries?: Record<string, ManifestBinary | undefined>;
}

// Injectable HTTP GET seam: production uses https.get; tests swap in a local
// http server so redirect/status/timeout logic runs without external network.
type GetFn = (target: string, callback: (res: http.IncomingMessage) => void) => http.ClientRequest;
let getImpl: GetFn = (target, callback) => https.get(target, callback);

export function __setHttpGet(fn: GetFn | null): void {
    getImpl = fn ?? ((target, callback) => https.get(target, callback));
}

/**
 * Returns true when the host exposes the MCP server definition provider API.
 * Typed loosely so it can be unit-tested with a fake API object.
 */
export function isMcpApiAvailable(api: any): boolean {
    return typeof api?.lm?.registerMcpServerDefinitionProvider === "function"
        && typeof api?.McpStdioServerDefinition === "function";
}

/**
 * Builds the command/args used to launch the Topos MCP server from a resolved path.
 * A Python interpreter path runs the module form; a native binary runs directly.
 */
export function buildServerInvocation(resolvedBinaryPath: string): { command: string; args: string[] } {
    if (resolvedBinaryPath.endsWith("python") || resolvedBinaryPath.endsWith("python3")) {
        return { command: resolvedBinaryPath, args: ["-m", "topos.cli", "mcp"] };
    }
    return { command: resolvedBinaryPath, args: ["mcp"] };
}

/**
 * Builds argv for a Topos CLI subcommand (evaluate, depgraph, etc.) from the same
 * resolved executable used for MCP (native binary or python -m topos.cli).
 */
export function buildCliInvocation(resolvedBinaryPath: string, cliArgs: string[]): { command: string; args: string[] } {
    const { command, args } = buildServerInvocation(resolvedBinaryPath);
    const withoutMcp = args[args.length - 1] === "mcp" ? args.slice(0, -1) : args;
    return { command, args: [...withoutMcp, ...cliArgs] };
}

/**
 * Map Node.js platform and architecture fields to the remote manifest platform keys.
 */
export function getPlatformKey(platform: NodeJS.Platform = process.platform, arch: string = process.arch): string | undefined {
    if (platform === 'darwin') {
        return arch === 'arm64' ? 'darwin-arm64' : undefined;
    } else if (platform === 'linux') {
        return arch === 'arm64' ? 'linux-arm64' : 'linux-x64';
    }
    return undefined;
}

/**
 * Resolves paths starting with tilde (~) to the user's home directory.
 */
export function resolveHomePath(filePath: string): string {
    if (filePath.startsWith('~')) {
        const homeDir = process.env.HOME || process.env.USERPROFILE || "";
        const relative = filePath === '~' ? '' : filePath.startsWith('~/') ? filePath.slice(2) : filePath.slice(1);
        return relative ? path.join(homeDir, relative) : homeDir;
    }
    return path.resolve(filePath);
}

/**
 * Selects the manifest binary entry for a given platform key.
 */
export function selectBinaryFromManifest(manifest: ReleaseManifest | undefined, platformKey: string | undefined): ManifestBinary | undefined {
    if (!manifest || !platformKey) {
        return undefined;
    }
    return manifest.binaries?.[platformKey];
}

/**
 * Computes the SHA-256 hash of a local file.
 */
export function computeFileSha256(filePath: string): Promise<string> {
    return new Promise((resolve, reject) => {
        const hash = crypto.createHash('sha256');
        const stream = fs.createReadStream(filePath);

        stream.on('error', err => reject(err));
        stream.on('data', chunk => hash.update(chunk));
        stream.on('end', () => resolve(hash.digest('hex')));
    });
}

/**
 * Fetches JSON payload from a remote URL, following HTTP/HTTPS redirects up to
 * MAX_REDIRECTS and aborting requests that stall past REQUEST_TIMEOUT_MS.
 */
export function fetchJson(targetUrl: string, redirectsRemaining: number = MAX_REDIRECTS): Promise<any> {
    return new Promise((resolve, reject) => {
        const request = getImpl(targetUrl, (response) => {
            const statusCode = response.statusCode ?? 0;

            if (statusCode >= 300 && statusCode < 400 && response.headers.location) {
                response.resume();
                if (redirectsRemaining <= 0) {
                    reject(new Error(`Too many redirects while fetching JSON from ${targetUrl}`));
                    return;
                }
                const nextUrl = url.resolve(targetUrl, response.headers.location);
                fetchJson(nextUrl, redirectsRemaining - 1).then(resolve).catch(reject);
                return;
            }

            if (statusCode !== 200) {
                response.resume();
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
        });

        request.setTimeout(REQUEST_TIMEOUT_MS, () => {
            request.destroy(new Error(`Request timed out after ${REQUEST_TIMEOUT_MS}ms while fetching JSON from ${targetUrl}`));
        });
        request.on('error', err => reject(err));
    });
}

/**
 * Downloads a file from a URL to a local destination path, following redirects up
 * to MAX_REDIRECTS and aborting requests that stall past REQUEST_TIMEOUT_MS.
 */
export function downloadFile(
    targetUrl: string,
    destPath: string,
    onProgress?: (fraction: number) => void,
    redirectsRemaining: number = MAX_REDIRECTS
): Promise<void> {
    return new Promise((resolve, reject) => {
        const file = fs.createWriteStream(destPath);

        const cleanup = () => {
            file.close();
            fs.unlink(destPath, () => {});
        };

        const request = getImpl(targetUrl, (response) => {
            const statusCode = response.statusCode ?? 0;

            if (statusCode >= 300 && statusCode < 400 && response.headers.location) {
                response.resume();
                cleanup();
                if (redirectsRemaining <= 0) {
                    reject(new Error(`Too many redirects while downloading ${targetUrl}`));
                    return;
                }
                const nextUrl = url.resolve(targetUrl, response.headers.location);
                downloadFile(nextUrl, destPath, onProgress, redirectsRemaining - 1).then(resolve).catch(reject);
                return;
            }

            if (statusCode !== 200) {
                response.resume();
                cleanup();
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
                cleanup();
                reject(err);
            });
        });

        request.setTimeout(REQUEST_TIMEOUT_MS, () => {
            request.destroy(new Error(`Request timed out after ${REQUEST_TIMEOUT_MS}ms while downloading ${targetUrl}`));
        });
        request.on('error', (err) => {
            cleanup();
            reject(err);
        });
    });
}

/** Mirrors ``topos.graphs.ast.dispatch.SUPPORTED_LANGUAGES`` / ``LANGUAGE_FILE_SUFFIXES``. */
export const TOPOS_SUPPORTED_LANGUAGES = [
    'cpp',
    'javascript',
    'python',
    'rust',
    'typescript',
] as const;

export type ToposLanguage = (typeof TOPOS_SUPPORTED_LANGUAGES)[number];

const SUFFIX_TO_LANGUAGE: ReadonlyMap<string, ToposLanguage> = new Map([
    ['.py', 'python'],
    ['.rs', 'rust'],
    ['.js', 'javascript'],
    ['.mjs', 'javascript'],
    ['.cjs', 'javascript'],
    ['.ts', 'typescript'],
    ['.tsx', 'typescript'],
    ['.cpp', 'cpp'],
    ['.cc', 'cpp'],
    ['.cxx', 'cpp'],
    ['.hpp', 'cpp'],
    ['.hh', 'cpp'],
    ['.hxx', 'cpp'],
]);

/** Directory names skipped during language detection (aligned with common tooling ignores). */
export const LANGUAGE_DETECT_SKIP_DIRS = new Set([
    '.git',
    '.gitnexus',
    '.hg',
    '.svn',
    '.venv',
    'venv',
    'venv.bak',
    'env',
    '__pycache__',
    '__pypackages__',
    'node_modules',
    'dist',
    'build',
    'out',
    'target',
    '.next',
    '.turbo',
    'coverage',
    'htmlcov',
    '.pytest_cache',
    '.mypy_cache',
    '.tox',
    '.ruff_cache',
    '.eggs',
    '.pixi',
]);

/**
 * Walk *rootDir* (recursive) and return supported Topos languages present, in stable
 * pillar order. Stops after *maxFiles* regular files for responsiveness on huge trees.
 */
export function detectLanguagesInTree(
    rootDir: string,
    options: { maxFiles?: number } = {}
): ToposLanguage[] {
    const maxFiles = options.maxFiles ?? 20_000;
    const found = new Set<ToposLanguage>();
    let scanned = 0;

    const stack: string[] = [rootDir];
    while (stack.length > 0 && scanned < maxFiles) {
        const dir = stack.pop()!;
        let entries: fs.Dirent[];
        try {
            entries = fs.readdirSync(dir, { withFileTypes: true });
        } catch {
            continue;
        }

        for (const entry of entries) {
            if (scanned >= maxFiles) {
                break;
            }
            const full = path.join(dir, entry.name);
            if (entry.isDirectory()) {
                if (LANGUAGE_DETECT_SKIP_DIRS.has(entry.name)) {
                    continue;
                }
                stack.push(full);
            } else if (entry.isFile()) {
                scanned += 1;
                const lang = SUFFIX_TO_LANGUAGE.get(path.extname(entry.name).toLowerCase());
                if (lang) {
                    found.add(lang);
                    if (found.size === TOPOS_SUPPORTED_LANGUAGES.length) {
                        break;
                    }
                }
            }
        }
    }

    return TOPOS_SUPPORTED_LANGUAGES.filter((lang) => found.has(lang));
}

/**
 * Resolve languages for **Evaluate Project**: explicit *configuredLanguage* when not
 * ``auto``, otherwise scan *scanRoot* on disk.
 */
export function resolveEvaluateLanguages(
    scanRoot: string,
    configuredLanguage: string
): ToposLanguage[] {
    const trimmed = configuredLanguage.trim().toLowerCase();
    if (trimmed && trimmed !== 'auto') {
        if ((TOPOS_SUPPORTED_LANGUAGES as readonly string[]).includes(trimmed)) {
            return [trimmed as ToposLanguage];
        }
        return [];
    }
    return detectLanguagesInTree(scanRoot);
}
