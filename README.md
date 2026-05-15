# Topos

> **Structural code quality metrics your agents can act on.**

**Assume you passed the tests. How good is your solution?**

Topos fills the gap between _correctness_ (passing tests) and _quality_ (maintainable, secure, and well-structured code). It provides structural metrics that coding agents use to iteratively optimize code until it earns the highest possible **Code Quality Badge**.

Three independent quality pillars:

- **SIMPLE:** The code is readable and structurally predictable. Evaluates CFG complexity and AST entropy.
- **COMPOSABLE:** The module is cleanly decoupled. Evaluates Martin coupling and instability (requires GitNexus).
- **SECURE:** The data flow is safe. Evaluates dangerous-API reachability and taint paths.

Set a **Preference Ranking** (e.g., `simple,composable,secure`) to define how an agent should prioritize these pillars when time or tokens are limited.

> [!NOTE]
> We model programs as maps (morphisms) on graphs. This lets us evaluate design properties that go beyond preserving inputs and outputs (correctness).

---

### Badges for Coding Agents

Topos maps every file to a **Code Quality Badge** on an eight-valued evaluation lattice. Agents always know exactly where they are:

```mermaid
graph BT
    SLOP["⊥ SLOP<br/> No badges met"]
    SIMPLE["S<br/>Simple"]
    COMPOSABLE["C<br/>Composable"]
    SECURE["Sc<br/>Secure"]
    SC["S∧C"]
    SSc["S∧Sc"]
    CSc["C∧Sc"]
    IDEAL["⊤ IDEAL<br/> All three pillars achieved"]

    SLOP --> SIMPLE
    SLOP --> COMPOSABLE
    SLOP --> SECURE
    SIMPLE --> SC
    SIMPLE --> SSc
    COMPOSABLE --> SC
    COMPOSABLE --> CSc
    SECURE --> SSc
    SECURE --> CSc
    SC --> IDEAL
    SSc --> IDEAL
    CSc --> IDEAL

    style SLOP       fill:#f8d7da,stroke:#842029,color:#000
    style SIMPLE     fill:#d4edda,stroke:#155724,color:#000
    style COMPOSABLE fill:#d1ecf1,stroke:#0c5460,color:#000
    style SECURE     fill:#d1f1dc,stroke:#0c5460,color:#000
    style SC         fill:#e2f5eb,stroke:#155724,color:#000
    style SSc        fill:#e2f5eb,stroke:#155724,color:#000
    style CSc        fill:#e2f5eb,stroke:#155724,color:#000
    style IDEAL      fill:#fff3cd,stroke:#856404,color:#000
```

**SIMPLE**, **COMPOSABLE**, and **SECURE** are **pairwise incomparable** — code can achieve any subset independently. **IDEAL** is the intersection of all three.

> [!TIP]
> Perfect code reaches **IDEAL** — but agents operate under token and time budgets. As a manager, you set the **Preference Ranking** to tell the agent how to relax its goals if the ideal target is unfeasible. Set `--preferences simple,composable,secure` to prioritize simplicity first.

---

### Quick Start

#### Install

```bash
curl -sSL https://raw.githubusercontent.com/Krv-Labs/topos/main/install.sh | sh
```

#### CLI

```bash
topos evaluate src/ -r --preferences simple,composable,secure  # classify a directory
topos evaluate src/ -r --gitnexus-dir .gitnexus --preferences simple,composable,secure  # with coupling & security
topos inspect module.py --preferences simple,composable,secure # detailed metrics
topos structural-test-coverage src/ --language python         # measure test code coverage
topos compare before.py after.py                              # AST edit distance
```

#### In an agent loop

```
Agent iteration 1: SLOP [simple: 41%, composable: -, secure: -]
  → Reduce cyclomatic complexity and normalize entropy toward 0.5

Agent iteration 2: SIMPLE [simple: 72%, composable: -, secure: -]
  → ✓ SIMPLE badge earned.

Agent iteration 3: SIMPLE_COMPOSABLE [simple: 72%, composable: 65%, secure: -]
  → ✓ SIMPLE_COMPOSABLE badge earned. (With GitNexus enabled)
```

---

### MCP Server

Give any MCP-compatible agent — Claude Code, Cursor, Gemini CLI, Windsurf — a live feed of Topos verdicts so it can evaluate and iterate on its own output.

<details>
<summary><b>Set up <code>topos-mcp</code> in your agent</b></summary>

&nbsp;

#### Step 1 — Build the dependency graph (optional but recommended)

> [!IMPORTANT]
> **Recommended.** Without a dependency graph, Topos cannot score COMPOSABLE — any verdict containing it (including `IDEAL`) is unreachable. `SIMPLE` and `SECURE` always run.
>
> ```bash
> npm install -g gitnexus        # one-time per machine
> cd /path/to/your/repo
> topos depgraph generate        # one-time per repo; writes .gitnexus/
> ```
>
> Re-run when imports change (new modules, renames, restructures). The cache keys on `.gitnexus/` mtime and invalidates itself.

> [!TIP]
> Verify the binary before wiring it into editors:
>
> ```bash
> topos-mcp   # prints the FastMCP banner and waits on stdin. Ctrl-C to exit.
> ```

#### Step 2 — Register with your agent

Run from your project root — Topos auto-detects its file-access root by walking up for `.git` or `pyproject.toml`.

##### Claude Code

```bash
claude mcp add topos topos-mcp
```

##### Gemini CLI

```bash
gemini mcp add topos topos-mcp
```

##### Cursor

<a href="cursor://anysphere.cursor-deeplink/mcp/install?name=topos&config=eyJjb21tYW5kIjogInRvcG9zLW1jcCJ9">**➕ Install `topos` in Cursor**</a>

Or edit `.cursor/mcp.json`:

```json
{ "mcpServers": { "topos": { "command": "topos-mcp" } } }
```

##### Windsurf and everything else

```json
{ "mcpServers": { "topos": { "command": "topos-mcp" } } }
```

#### Step 3 — Launch from the project root

> [!IMPORTANT]
> Topos refuses to read files outside a trusted root. If you must launch from elsewhere, set it explicitly:
>
> ```json
> {
>   "command": "topos-mcp",
>   "env": { "TOPOS_MCP_FILE_ROOT": "/absolute/path/to/repo" }
> }
> ```

> [!TIP]
> On the agent's first turn, point it at the workflow doc:
>
> > "Fetch `topos://docs/workflows` and follow the Topos refactor loop."
>
> Or invoke the prompt directly: `topos_refactor_until_ideal(filepath="path/to/file.py")`.

#### Smoke test

> "Use topos to find the worst-scoring file in `src/`, propose a refactor, and verify with `topos_assess_improvement`."

A healthy response shows `{simple: 72%, composable: 65%, secure: 95%}` when GitNexus is configured. If the response is missing `composable`, go back to Step 1.

</details>

---

### Contributing

Topos is used internally at [Krv Labs](https://krv.ai) to manage AI agent code output. We welcome bugs, ideas, and contributions.

- **Bug?** Open an [Issue](https://github.com/Krv-Labs/topos/issues)
- **Idea?** Start a [Discussion](https://github.com/Krv-Labs/topos/discussions) or open a PR
- **Collaborate?** [team@krv.ai](mailto:team@krv.ai)

---

[Full Documentation](docs/) · [Measures & Metrics](docs/source/measures.rst) · [Category Theory Concepts](docs/source/concepts.rst)

_Built with ❤️ by [Krv Labs](https://krv.ai)_
