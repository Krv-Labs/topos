import { test } from 'node:test';
import assert from 'node:assert/strict';
import * as os from 'os';
import * as fs from 'fs';
import * as path from 'path';
import * as http from 'http';
import * as crypto from 'crypto';
import { AddressInfo } from 'net';

import {
    getPlatformKey,
    resolveHomePath,
    computeFileSha256,
    selectBinaryFromManifest,
    fetchJson,
    downloadFile,
    isMcpApiAvailable,
    buildServerInvocation,
    buildCliInvocation,
    detectLanguagesInTree,
    resolveEvaluateLanguages,
    __setHttpGet,
    REQUEST_TIMEOUT_MS,
} from '../../runtime';

// --- getPlatformKey -------------------------------------------------------

test('getPlatformKey maps every supported platform/arch pair', () => {
    assert.equal(getPlatformKey('darwin', 'arm64'), 'darwin-arm64');
    assert.equal(getPlatformKey('linux', 'arm64'), 'linux-arm64');
    assert.equal(getPlatformKey('linux', 'x64'), 'linux-x64');
});

test('getPlatformKey treats non-arm64 linux as x64', () => {
    assert.equal(getPlatformKey('linux', 'ppc64'), 'linux-x64');
});

test('getPlatformKey returns undefined for unsupported platforms', () => {
    assert.equal(getPlatformKey('darwin', 'x64'), undefined);
    assert.equal(getPlatformKey('darwin', 'ia32'), undefined);
    assert.equal(getPlatformKey('win32', 'x64'), undefined);
    assert.equal(getPlatformKey('freebsd', 'x64'), undefined);
});

// --- resolveHomePath ------------------------------------------------------

test('resolveHomePath expands a leading tilde to the home directory', () => {
    const previous = process.env.HOME;
    process.env.HOME = '/home/tester';
    try {
        assert.equal(resolveHomePath('~/bin/topos'), '/home/tester/bin/topos');
        assert.equal(resolveHomePath('~'), '/home/tester');
    } finally {
        if (previous === undefined) delete process.env.HOME; else process.env.HOME = previous;
    }
});

test('resolveHomePath returns absolute paths unchanged', () => {
    assert.equal(resolveHomePath('/usr/local/bin/topos'), '/usr/local/bin/topos');
});

test('resolveHomePath resolves relative paths against cwd', () => {
    assert.equal(resolveHomePath('bin/topos'), path.resolve('bin/topos'));
});

// --- selectBinaryFromManifest ---------------------------------------------

test('selectBinaryFromManifest returns the entry for a present platform', () => {
    const manifest = { binaries: { 'darwin-arm64': { url: 'https://x/y', sha256: 'abc' } } };
    assert.deepEqual(selectBinaryFromManifest(manifest, 'darwin-arm64'), { url: 'https://x/y', sha256: 'abc' });
});

test('selectBinaryFromManifest returns undefined for a missing platform or empty input', () => {
    const manifest = { binaries: { 'darwin-arm64': { url: 'https://x/y', sha256: 'abc' } } };
    assert.equal(selectBinaryFromManifest(manifest, 'linux-x64'), undefined);
    assert.equal(selectBinaryFromManifest(manifest, undefined), undefined);
    assert.equal(selectBinaryFromManifest(undefined, 'darwin-arm64'), undefined);
});

// --- computeFileSha256 ----------------------------------------------------

test('computeFileSha256 matches a known digest', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'topos-sha-'));
    const file = path.join(dir, 'payload.bin');
    const contents = 'topos-code-quality';
    fs.writeFileSync(file, contents);
    try {
        const expected = crypto.createHash('sha256').update(contents).digest('hex');
        assert.equal(await computeFileSha256(file), expected);
    } finally {
        fs.rmSync(dir, { recursive: true, force: true });
    }
});

// --- isMcpApiAvailable ----------------------------------------------------

test('isMcpApiAvailable is true only when both API surfaces exist', () => {
    const full = { lm: { registerMcpServerDefinitionProvider: () => {} }, McpStdioServerDefinition: function () {} };
    assert.equal(isMcpApiAvailable(full as any), true);
});

test('isMcpApiAvailable is false when lm or McpStdioServerDefinition is missing', () => {
    assert.equal(isMcpApiAvailable({} as any), false);
    assert.equal(isMcpApiAvailable({ lm: {} } as any), false);
    assert.equal(isMcpApiAvailable({ lm: { registerMcpServerDefinitionProvider: () => {} } } as any), false);
    assert.equal(isMcpApiAvailable({ McpStdioServerDefinition: function () {} } as any), false);
});

// --- buildServerInvocation ------------------------------------------------

test('buildServerInvocation runs a native binary directly', () => {
    assert.deepEqual(buildServerInvocation('/opt/topos/bin/topos'), { command: '/opt/topos/bin/topos', args: ['mcp'] });
});

test('buildServerInvocation runs a python interpreter via the module form', () => {
    assert.deepEqual(buildServerInvocation('/venv/bin/python'), { command: '/venv/bin/python', args: ['-m', 'topos.cli', 'mcp'] });
    assert.deepEqual(buildServerInvocation('/usr/bin/python3'), { command: '/usr/bin/python3', args: ['-m', 'topos.cli', 'mcp'] });
});

test('buildCliInvocation replaces mcp subcommand with CLI args', () => {
    assert.deepEqual(
        buildCliInvocation('/opt/topos/bin/topos', ['evaluate', 'src', '-r', '-v']),
        { command: '/opt/topos/bin/topos', args: ['evaluate', 'src', '-r', '-v'] }
    );
    assert.deepEqual(
        buildCliInvocation('/venv/bin/python', ['depgraph', 'generate']),
        { command: '/venv/bin/python', args: ['-m', 'topos.cli', 'depgraph', 'generate'] }
    );
});

// --- detectLanguagesInTree / resolveEvaluateLanguages -----------------------

test('detectLanguagesInTree skips .venv when scanning for languages', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'topos-venv-'));
    try {
        fs.writeFileSync(path.join(root, 'app.py'), 'x = 1\n');
        fs.mkdirSync(path.join(root, '.venv', 'lib'), { recursive: true });
        fs.writeFileSync(path.join(root, '.venv', 'lib', 'site.py'), 'print(1)\n');
        assert.deepEqual(detectLanguagesInTree(root), ['python']);
    } finally {
        fs.rmSync(root, { recursive: true, force: true });
    }
});

test('detectLanguagesInTree finds multiple languages and skips node_modules', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'topos-lang-'));
    try {
        fs.mkdirSync(path.join(root, 'src'), { recursive: true });
        fs.writeFileSync(path.join(root, 'src', 'app.py'), 'x = 1\n');
        fs.writeFileSync(path.join(root, 'src', 'ui.ts'), 'export {};\n');
        fs.mkdirSync(path.join(root, 'node_modules', 'pkg'), { recursive: true });
        fs.writeFileSync(path.join(root, 'node_modules', 'pkg', 'ignored.js'), 'module.exports = {};\n');
        assert.deepEqual(detectLanguagesInTree(root), ['python', 'typescript']);
    } finally {
        fs.rmSync(root, { recursive: true, force: true });
    }
});

test('resolveEvaluateLanguages honors explicit language and auto mode', () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'topos-lang-'));
    try {
        fs.writeFileSync(path.join(root, 'main.rs'), 'fn main() {}\n');
        assert.deepEqual(resolveEvaluateLanguages(root, 'rust'), ['rust']);
        assert.deepEqual(resolveEvaluateLanguages(root, 'auto'), ['rust']);
        assert.deepEqual(resolveEvaluateLanguages(root, ''), ['rust']);
        assert.deepEqual(resolveEvaluateLanguages(root, 'java'), []);
    } finally {
        fs.rmSync(root, { recursive: true, force: true });
    }
});

// --- fetchJson / downloadFile (against a local HTTP server) ----------------
//
// runtime.ts routes requests through an injectable GET seam (__setHttpGet). We
// point it at a local http server for the duration of each test so the real
// redirect, status, and timeout logic runs without external network access.

function withHttpServer(handler: http.RequestListener, fn: (baseUrl: string) => Promise<void>): Promise<void> {
    return new Promise((resolve, reject) => {
        const server = http.createServer(handler);
        server.listen(0, '127.0.0.1', async () => {
            const { port } = server.address() as AddressInfo;
            const baseUrl = `http://127.0.0.1:${port}`;
            __setHttpGet((target, callback) => http.get(target, callback));
            try {
                await fn(baseUrl);
                resolve();
            } catch (err) {
                reject(err);
            } finally {
                __setHttpGet(null);
                server.close();
            }
        });
    });
}

test('fetchJson parses a 200 JSON body', async () => {
    await withHttpServer((_req, res) => {
        res.writeHead(200, { 'content-type': 'application/json' });
        res.end(JSON.stringify({ latest_cli_version: '1.2.3' }));
    }, async (baseUrl) => {
        const json = await fetchJson(baseUrl);
        assert.equal(json.latest_cli_version, '1.2.3');
    });
});

test('fetchJson rejects on non-200 status', async () => {
    await withHttpServer((_req, res) => {
        res.writeHead(500);
        res.end('boom');
    }, async (baseUrl) => {
        await assert.rejects(() => fetchJson(baseUrl), /status code 500/);
    });
});

test('fetchJson follows redirects', async () => {
    await withHttpServer((req, res) => {
        if (req.url === '/start') {
            res.writeHead(302, { location: '/final' });
            res.end();
            return;
        }
        res.writeHead(200);
        res.end(JSON.stringify({ ok: true }));
    }, async (baseUrl) => {
        const json = await fetchJson(`${baseUrl}/start`);
        assert.equal(json.ok, true);
    });
});

test('fetchJson rejects when the redirect cap is exceeded', async () => {
    let hops = 0;
    await withHttpServer((_req, res) => {
        hops += 1;
        res.writeHead(302, { location: `/hop-${hops}` });
        res.end();
    }, async (baseUrl) => {
        await assert.rejects(() => fetchJson(`${baseUrl}/start`), /Too many redirects/);
    });
});

test('fetchJson aborts a stalled request via the request timeout', { timeout: REQUEST_TIMEOUT_MS + 5000 }, async () => {
    await withHttpServer((_req, _res) => {
        // Never respond; rely on the request timeout to fire.
    }, async (baseUrl) => {
        await assert.rejects(() => fetchJson(baseUrl), /timed out/);
    });
});

test('downloadFile writes the body and follows redirects', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'topos-dl-'));
    const dest = path.join(dir, 'topos-cli');
    try {
        await withHttpServer((req, res) => {
            if (req.url === '/start') {
                res.writeHead(302, { location: '/final' });
                res.end();
                return;
            }
            res.writeHead(200, { 'content-length': '5' });
            res.end('hello');
        }, async (baseUrl) => {
            await downloadFile(`${baseUrl}/start`, dest);
            assert.equal(fs.readFileSync(dest, 'utf8'), 'hello');
        });
    } finally {
        fs.rmSync(dir, { recursive: true, force: true });
    }
});

test('downloadFile rejects on non-200 and leaves no file behind', async () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'topos-dl-'));
    const dest = path.join(dir, 'topos-cli');
    try {
        await withHttpServer((_req, res) => {
            res.writeHead(404);
            res.end();
        }, async (baseUrl) => {
            await assert.rejects(() => downloadFile(baseUrl, dest), /status code 404/);
            assert.equal(fs.existsSync(dest), false);
        });
    } finally {
        fs.rmSync(dir, { recursive: true, force: true });
    }
});
