# `.mcp/server.json` → VS Code install failure: investigation report

**Date:** 2026-08-03
**Branch:** `fix/mcp-server-json-index-url` (worktree at `/Users/gathrid/Repos/topos-mcp-indexurl`, off `main` @ `4f8e63a`)
**Symptom:** installing `io.github.Krv-Labs/topos` via the VS Code `@mcp` extension writes a config whose `uvx` invocation hard-fails:

```
error: the argument '--index-url <INDEX_URL>' cannot be used multiple times
Process exited with code 2
```

---

## 1. Headline

**Both fields in the current package record are wrong, and fixing either one alone still leaves the install broken.**

| Field | Current value | Verdict |
| --- | --- | --- |
| `registryBaseUrl` | `https://pypi.org` | Only publishable value — but it is **not a PEP 503 index**, so the flag VS Code builds from it cannot resolve the package. |
| `runtimeArguments` → `--index-url https://pypi.org/simple` | added in #252 | Correct URL, but it **duplicates** a flag VS Code already injects. `uv` rejects a repeated `--index-url` at argument-parse time. |

The fix is to **delete both**. `registryBaseUrl` is optional; omitted, VS Code injects nothing and runs `uvx topos-mcp@<version>`, which resolves against uv's default index (`https://pypi.org/simple`).

The premise that drove #252 — "publishing needs `https://pypi.org`, installing needs `/simple`" — is **true on both halves**, but the conclusion (supply both) was wrong. The third option, supply neither, satisfies both constraints.

---

## 2. Evidence

### 2.1 VS Code injects `--index-url` from `registryBaseUrl`, unconditionally

`microsoft/vscode` @ `219ad590fe97`, `src/vs/platform/mcp/common/mcpManagementService.ts:136-141`:

```ts
case RegistryType.PYTHON:
    if (serverPackage.registryBaseUrl) {
        args.push('--index-url', serverPackage.registryBaseUrl);
    }
    args.push(serverPackage.version ? `${serverPackage.identifier}@${serverPackage.version}` : serverPackage.identifier);
    break;
```

A bare truthy check. There is **no** default-registry constant and **no** skip-if-default logic anywhere in the repo (`gh api search/code` for `defaultRegistry` → 0 results).

Arg order is `runtimeArguments` (:109) → `environmentVariables` (:116) → injected `--index-url` (:137) → `identifier@version` (:140) → `packageArguments` (:162). That maps element-for-element onto the observed output:

| index | value | source |
| --- | --- | --- |
| 0,1 | `--index-url`, `https://pypi.org/simple` | our `runtimeArguments` |
| 2,3 | `--index-url`, `https://pypi.org` | VS Code's injection from `registryBaseUrl` |
| 4 | `topos-mcp@0.4.3` | line 140 |

Maintainer position (@sandy081, microsoft/vscode#282860, closed invalid):

> "As per the MCP registry spec you have to use `registryBaseUrl` property to provide the custom registry. **Client will decide how to run the given MCP server and the package configuration should not hardcode that.**"

That is exactly the mistake #252 made.

Related: microsoft/vscode#294659 ("URL Schema not trimmed from `registryBaseUrl`") confirms "the registryBaseUrl is just passed verbatim" — closed for lack of upvotes, **not fixed**.

### 2.2 `uv` rejects a repeated `--index-url`, and the bare host does not resolve

Local, uv `0.9.1`:

```
$ uvx --index-url https://pypi.org/simple --index-url https://pypi.org topos-mcp@0.4.3 --version
error: the argument '--index-url <INDEX_URL>' cannot be used multiple times
EXIT=2                                            # clap parse failure

$ uvx --index-url https://pypi.org topos-mcp@0.4.3 --version
  × No solution found when resolving tool dependencies:
  ╰─▶ Because topos-mcp was not found in the package registry ...
EXIT=1                                            # resolution failure

$ uvx --index-url https://pypi.org/simple topos-mcp@0.4.3 --version
EXIT=0                                            # works

$ uvx topos-mcp@0.4.3 --version                   # no flag at all
EXIT=0                                            # works (also verified with --no-cache)
```

Why the bare host fails — `uv -v` shows it derives `https://pypi.org/topos-mcp/`:

- `curl -sI https://pypi.org/topos-mcp/` → **404**, no `location:` header. There is **no redirect** to `/simple/`.
- `curl -sI https://pypi.org/simple/topos-mcp/` → **200**.

PEP 503: *"The API is named the 'simple' repository due to the fact that PyPI's base URL is `https://pypi.org/simple/`."* The bare host is the human web UI, not an index. uv's docs note it "will always continue searching across indexes when it encounters a 404 Not Found" — which is why the mistake surfaces as a confusing resolver error rather than an HTTP error.

**So even without the duplicate, `--index-url https://pypi.org` alone would break the install.** That is the fact that kills the "just drop `runtimeArguments`" fix.

### 2.3 The registry rejects `/simple` at publish — confirmed in source

`modelcontextprotocol/registry`, `internal/validators/registries/pypi.go:28-53`:

```go
func ValidatePyPI(ctx context.Context, pkg model.Package, serverName string) error {
	// Set default registry base URL if empty
	if pkg.RegistryBaseURL == "" {
		pkg.RegistryBaseURL = model.RegistryURLPyPI
	}
	...
	if pkg.RegistryBaseURL != model.RegistryURLPyPI {
		return fmt.Errorf("registry type and base URL do not match: '%s' is not valid for registry type '%s'. Expected: %s", ...)
	}
```

with `pkg/model/constants.go:19` → `RegistryURLPyPI = "https://pypi.org"`.

Exact string compare. No normalization — even a trailing slash fails. Docs corroborate (`docs/modelcontextprotocol-io/package-types.mdx:54`): *"For PyPI packages, the MCP Registry currently supports the official PyPI registry (`https://pypi.org`) only."*

**Your recollection was right.** `/simple` is unpublishable.

### 2.4 …but omitting the field is publishable, and keeps the served record clean

Two independent facts make the omit-fix work:

1. **`registryBaseUrl` is optional.** The `2025-12-11` schema's `Package.required` is `["registryType","identifier","transport"]`. There is no `enum` and no `pattern` on `registryBaseUrl` — only non-normative `examples`. (A stale contributor doc, `docs/contributing/add-package-registry.md`, says to add values to a `registryBaseUrl` *enum*; no such enum exists. That doc is a plausible origin of the belief that the schema constrains it.)

2. **The default-fill does not reach the stored record.** `ValidatePyPI` takes `pkg model.Package` **by value** — the `pkg.RegistryBaseURL = model.RegistryURLPyPI` assignment mutates a local copy, used only to build the ownership-check URL (`{base}/pypi/{name}/{version}/json`). The published record keeps the field absent.

Confirmed live: two published PyPI servers serve no `registryBaseUrl` at all —

```
ai.adeu/adeu            | baseUrl=None
ai.anomalyarmor/armor-mcp | baseUrl=None
```

### 2.5 `registryType` is load-bearing; `runtimeHint` is not

`command` comes from a hardcoded map, `mcpManagementService.ts:174` → `:183-191`:

```ts
protected getCommandName(packageType: RegistryType): string {
	switch (packageType) {
		case RegistryType.NODE: return 'npx';
		case RegistryType.DOCKER: return 'docker';
		case RegistryType.PYTHON: return 'uvx';
		case RegistryType.NUGET: return 'dnx';
	}
	return packageType;
}
```

`grep -n "runtimeHint"` against that file returns **zero matches** — VS Code deserializes it (`mcpGalleryService.ts:407`) and never reads it. `"uvx"` follows from `registryType: "pypi"` alone.

We keep `runtimeHint: "uvx"` anyway: it is schema-recommended and other clients may honor it. But **`registryType: "pypi"` is the field that must not change.**

### 2.6 `mcp-publisher validate` is not a publish preflight

Run locally against all three shapes:

| shape | `mcp-publisher validate` | actually publishable? |
| --- | --- | --- |
| current (`https://pypi.org` + duplicate arg) | ✅ valid | yes (and broken at install) |
| `https://pypi.org/simple` | ✅ valid | **no** — rejected by `pypi.go` |
| omitted (the fix) | ✅ valid | yes |

`internal/api/handlers/v0/validate.go:28` calls only `ValidateServerJSON(...)`; the base-URL guard and ownership check live in the publish path and are never invoked. **A green validate proves nothing about publishability.**

---

## 3. The fix (committed)

`3073ccc` on `fix/mcp-server-json-index-url`:

```diff
       "registryType": "pypi",
-      "registryBaseUrl": "https://pypi.org",
       "identifier": "topos-mcp",
       "version": "0.4.3",
       "runtimeHint": "uvx",
-      "runtimeArguments": [
-        {
-          "type": "named",
-          "name": "--index-url",
-          "value": "https://pypi.org/simple"
-        }
-      ],
       "transport": {
```

Plus a correction to `extensions/vscode/workflow/mcp-registry-publishing.md`, which documented the unpublishable `registryBaseUrl: "https://pypi.org/simple"` at line 90 — almost certainly the origin of this loop.

`scripts/check_versions.py` passes.

**Resulting VS Code config**, traced through every `args.push` site with both fields absent:

```json
"io.github.Krv-Labs/topos": {
  "type": "stdio",
  "command": "uvx",
  "args": ["topos-mcp@0.4.3"]
}
```

Verified end-to-end — a real MCP stdio handshake against that exact command:

```
$ echo '{"jsonrpc":"2.0","id":1,"method":"initialize",...}' | uvx topos-mcp@0.4.3
{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-06-18",
 "capabilities":{"prompts":{},"resources":{},"tools":{}},
 "serverInfo":{"name":"topos_mcp","version":"0.4.3+build.1785786694"}, ...}}
```

---

## 4. Ship path — this needs a release, not a metadata edit

Chain of constraints, each verified:

1. **Registry versions are immutable.** `internal/service/registry_service.go:188-194` → `CheckVersionExists` → `ErrInvalidVersion` → HTTP 400. Docs: *"Once published, the version string (and other metadata) cannot be changed."*
2. **No self-service edit.** `PUT /servers/{name}/versions/{version}` requires the `edit` permission; GitHub-AT, GitHub-OIDC, DNS and HTTP auth all issue **`publish` only**. `edit` is granted solely via operator-configured `OIDCEditPerms` or the anonymous namespace.
3. **`0.4.3` is already published and `isLatest: true`.** The broken record is live and cannot be replaced in place.
4. **The registry requires the PyPI version to exist first.** Release order is PyPI → registry.
5. **`scripts/check_versions.py:57-69`** requires `.mcp/server.json` top-level `version` *and every* `packages[].version` to equal the Cargo workspace version.

**Recommendation: fold this into the 0.4.4 release** already in flight on `release/044-256-harness-install`. That branch is still at Cargo `0.4.3`; the version bump happens at release time and will carry both `server.json` slots to `0.4.4`. Then publish manually — `release.yml` contains **no** `mcp-publisher` step, so registry publishing is the manual runbook flow.

Two follow-ups on the branch topology:

- The fix is on `fix/mcp-server-json-index-url` off `main` (`4f8e63a`), which is **one commit ahead** of the release branch. It needs to land somewhere it won't be stranded.
- `main` @ `4f8e63a` (`fix(packaging): drop redundant version stanza from Homebrew template`) is not on the release branch either.

<details>
<summary>Escape hatch if 0.4.4 is far out (not recommended)</summary>

Publish a registry record with server `version: "0.4.4"` while `packages[0].version` stays `"0.4.3"`. The spec permits the decoupling, `0.4.4 > 0.4.3` so it would be `isLatest`, and it needs **no** new PyPI release. Cost: `check_versions.py` must be relaxed, weakening the repo's parity invariant to save a release that is already happening.

A semver prerelease (`0.4.3-1`) does **not** work: published after `0.4.3`, it sorts *below* it and would not be marked `isLatest`.
</details>

---

## 5. Separate upstream item — `api.mcp.github.com` is serving degraded records

Independent of our record, and it will confuse verification after 0.4.4 ships.

The GitHub mirror's `/v0/servers` responses omit `registry_type` and send `registry_name: ""`. In `mcpGalleryService.ts:365-381`, `convertRegistryType(p.registry_type ?? p.registry_name)` then falls through `default:` → `RegistryType.NODE` → **`command: "npx"`**.

Observed directly: the `search` parameter is ignored entirely — `?search=topos`, `?search=Krv-Labs`, and `?name=io.github.Krv-Labs/topos` all return the identical unfiltered 30-server listing, and `io.github.Krv-Labs/topos` is not in it. `microsoft/markitdown` shows the same empty `registry_name` with `runtime_hint: "uvx"`, so this is mirror-wide, not topos-specific. Service health reports `status: "ready_for_testing"`, `sync.strategy: "v2"` — a migration in progress.

The official registry (`registry.modelcontextprotocol.io`) serves our record correctly and verbatim, including the `runtimeArguments` block — pure pass-through, no transform.

> **If a post-0.4.4 `@mcp` install emits `npx`, that is the mirror, not this fix.** Check the served `registry_type` before concluding the fix failed.
>
> The fix is strictly non-worsening on the degraded path too: today it would produce `npx --index-url https://pypi.org/simple topos-mcp@0.4.3`; after the fix, `npx topos-mcp@0.4.3`. Both wrong, but the latter becomes correct the moment the mirror restores `registry_type`.

---

## 6. Adjacent finding — the harness installer does not cover VS Code MCP

Not the reported bug, but it bears on "install into VS Code".

`topos install` (added in `158f837` on the release branch) is **unaffected** by any of the above: it writes `{"command": "topos", "args": ["mcp"]}` (`integrations.rs:108`), using the locally-installed binary rather than `uvx topos-mcp`. No index URL is involved anywhere in the crate.

However, the harness labeled **"Cursor & VS Code"** (`integrations.rs:51`, id `skills`) writes:

- the skill file to `~/.agents/skills/topos/SKILL.md`
- an MCP entry to **`~/.cursor/mcp.json`** (`configure.rs:294`)

`~/.cursor/mcp.json` is Cursor's path. **VS Code does not read it** — VS Code uses profile-level `mcp.json` under `~/Library/Application Support/Code/User/` or workspace `.vscode/mcp.json`. So for VS Code the installer delivers the skill file but no MCP registration, while the label implies both.

Either the label should narrow to "Cursor", or a VS Code MCP path should be added.

---

## 7. Unblock today

The `0.4.3` registry record cannot be fixed, so until 0.4.4 publishes, bypass the gallery. Replace the entry in your VS Code `mcp.json` with:

```json
"topos": {
  "type": "stdio",
  "command": "uvx",
  "args": ["topos-mcp@0.4.3"]
}
```

Verified working by the stdio handshake in §3. Note this drops the `gallery` and `version` keys the `@mcp` install added, and renames the key off the gallery id — both deliberate, so VS Code treats it as a manual entry rather than a gallery-managed one.
