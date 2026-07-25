# UAST Industry-Standard Alignment Reference

This document defines a practical Unified AST (UAST) schema for multi-language tooling and annotates each design choice with citations to commonly used industry AST standards.

## Scope

- Languages: Python, Rust, C++, JavaScript/TypeScript (Node.js ecosystem).
- Goal: preserve native AST fidelity while exposing a unified shape for cross-language analysis.
- Strategy: native parser output first, then map to UAST.

## Canonical Industry References

- Python `ast`: <https://docs.python.org/3/library/ast.html>
- Rust `syn`: <https://docs.rs/syn/latest/syn/>
- Clang AST (C/C++): <https://clang.llvm.org/docs/IntroductionToTheClangAST.html>
- ESTree spec (JavaScript): <https://github.com/estree/estree>
- TypeScript-ESTree bridge: <https://typescript-eslint.io/packages/typescript-estree/>
- Tree-sitter grammars and node model: <https://tree-sitter.github.io/tree-sitter/>

## TypeScript Schema (with citation comments)

```ts
/**
 * UAST core model
 * - Native-first, normalized-second architecture.
 * - Every node retains source span and native parser provenance.
 */
export type Language = "python" | "rust" | "cpp" | "javascript" | "typescript";

export interface SourceSpan {
  file: string;
  startByte: number;
  endByte: number;
  startLine: number;   // 1-based
  startColumn: number; // 0-based
  endLine: number;
  endColumn: number;
}

export interface NativeRef {
  parser: string;      // "cpython-ast" | "syn" | "clang" | "estree" | "typescript-estree"
  parserVersion: string;
  nodeKind: string;    // e.g. FunctionDef, ItemFn, CXXMethodDecl, FunctionDeclaration
  nativeId?: string;
}

export interface UNodeBase {
  id: string;
  kind: UNodeKind;
  lang: Language;
  span: SourceSpan;
  native: NativeRef;
  leadingComments?: CommentNode[];
  trailingComments?: CommentNode[];
  attributes?: Record<string, unknown>;
  // Citation: keeping location/provenance is standard for compiler/tooling diagnostics and refactoring.
  // Clang AST and Python ast both expose node location data; ESTree uses source locations in tooling ecosystems.
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://docs.python.org/3/library/ast.html
  // https://github.com/estree/estree
}

export type UNodeKind =
  | "File"
  | "ImportDecl"
  | "ExportDecl"
  | "NamespaceDecl"
  | "TypeDecl"
  | "FunctionDecl"
  | "MethodDecl"
  | "Param"
  | "BlockStmt"
  | "VarDecl"
  | "IfStmt"
  | "ForStmt"
  | "WhileStmt"
  | "MatchStmt"
  | "ReturnStmt"
  | "BreakStmt"
  | "ContinueStmt"
  | "ThrowStmt"
  | "TryStmt"
  | "ExprStmt"
  | "AssignExpr"
  | "BinaryExpr"
  | "UnaryExpr"
  | "CallExpr"
  | "MemberExpr"
  | "Identifier"
  | "Literal"
  | "TypeRef"
  | "Comment";

export interface FileNode extends UNodeBase {
  kind: "File";
  path: string;
  body: UNode[];
}

export interface IdentifierNode extends UNodeBase {
  kind: "Identifier";
  name: string;
}

export interface LiteralNode extends UNodeBase {
  kind: "Literal";
  literalKind: "string" | "number" | "boolean" | "null" | "char" | "bytes";
  value: string | number | boolean | null;
  // Citation: literal categories align with Python ast Constant/Str/Num (historical),
  // ESTree Literal, and typed literals represented in Rust/C++ parser ecosystems.
  // https://docs.python.org/3/library/ast.html
  // https://github.com/estree/estree
}

export interface TypeRefNode extends UNodeBase {
  kind: "TypeRef";
  name: string;
  genericArgs?: UNode[];
  // Citation: generic type arguments are first-class in Rust syn and C++ templates,
  // and are represented in TypeScript parser ASTs.
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://typescript-eslint.io/packages/typescript-estree/
}

export interface ParamNode extends UNodeBase {
  kind: "Param";
  name: IdentifierNode;
  type?: TypeRefNode;
  defaultValue?: UNode;
  isVariadic?: boolean;
  // Citation: parameter metadata (name/type/default/variadic) maps to
  // Python ast arguments, ESTree params/defaults/rest, Rust FnArg, and Clang ParamDecl.
  // https://docs.python.org/3/library/ast.html
  // https://github.com/estree/estree
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
}

export interface FunctionDeclNode extends UNodeBase {
  kind: "FunctionDecl";
  name: IdentifierNode;
  params: ParamNode[];
  returnType?: TypeRefNode;
  body?: BlockStmtNode;
  modifiers?: string[];
  typeParams?: TypeRefNode[];
  // Citation: function nodes are a universal top-level concept:
  // Python FunctionDef, Rust ItemFn, Clang FunctionDecl/CXXMethodDecl, ESTree FunctionDeclaration.
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://github.com/estree/estree
}

export interface MethodDeclNode extends UNodeBase {
  kind: "MethodDecl";
  ownerType: IdentifierNode;
  name: IdentifierNode;
  params: ParamNode[];
  returnType?: TypeRefNode;
  body?: BlockStmtNode;
  modifiers?: string[];
  // Citation: method/member function concepts map to class-member constructs in
  // Python ClassDef bodies, Rust impl methods, and Clang CXXMethodDecl.
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
}

export interface TypeDeclNode extends UNodeBase {
  kind: "TypeDecl";
  typeKind:
    | "class"
    | "struct"
    | "enum"
    | "interface"
    | "typeAlias"
    | "union"
    | "trait"
    | "abstractClass"
    | "protocol";
  // "trait" | "abstractClass" | "protocol" are abstract; the rest are
  // concrete. Drives Martin's Abstractness metric (mdg.abstractness,
  // crates/topos-core/src/functors/probes/uast/abstractness.rs) — see issue #124.
  name: IdentifierNode;
  members: UNode[];
  bases?: TypeRefNode[];
  typeParams?: TypeRefNode[];
  // Citation: normalized across ClassDef (Python), ItemStruct/ItemEnum (Rust),
  // CXXRecordDecl/EnumDecl (Clang), and ESTree+TS extensions for interfaces/types.
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://typescript-eslint.io/packages/typescript-estree/
}

export interface VarDeclNode extends UNodeBase {
  kind: "VarDecl";
  name: IdentifierNode;
  varKind: "const" | "let" | "var" | "static" | "auto" | "mut" | "immutable";
  declaredType?: TypeRefNode;
  init?: UNode;
  // Citation: declaration + initializer + optional annotation mirrors
  // Python Assign/AnnAssign, Rust let bindings, C++ VarDecl, ESTree VariableDeclaration.
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://github.com/estree/estree
}

export interface BlockStmtNode extends UNodeBase {
  kind: "BlockStmt";
  statements: UNode[];
}

export interface IfStmtNode extends UNodeBase {
  kind: "IfStmt";
  condition: UNode;
  thenBranch: BlockStmtNode;
  elseBranch?: BlockStmtNode | IfStmtNode;
  // Citation: direct mapping from If/elif/else forms in Python, Rust, C++, and JS ASTs.
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://github.com/estree/estree
}

export interface MatchStmtNode extends UNodeBase {
  kind: "MatchStmt";
  expression: UNode;
  arms: Array<{ pattern: UNode; guard?: UNode; body: UNode }>;
  // Citation: captures Rust match and Python match-case semantics in one normalized node.
  // For JS/C++ switch, map via attributes.matchFlavor = "switch" when needed.
  // https://docs.rs/syn/latest/syn/
  // https://docs.python.org/3/library/ast.html
}

export interface ReturnStmtNode extends UNodeBase {
  kind: "ReturnStmt";
  value?: UNode;
}

export interface CallExprNode extends UNodeBase {
  kind: "CallExpr";
  callee: UNode;
  args: UNode[];
  typeArgs?: TypeRefNode[];
  // Citation: call-expression shape is consistent across ESTree, Python ast Call,
  // Rust syn ExprCall/ExprMethodCall, and Clang call expression families.
  // https://github.com/estree/estree
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
}

export interface MemberExprNode extends UNodeBase {
  kind: "MemberExpr";
  object: UNode;
  property: IdentifierNode;
  accessKind: "dot" | "index" | "pointer";
  // Citation: aligns with ESTree MemberExpression, Python Attribute/Subscript,
  // and C++ member access operators ('.', '->').
  // https://github.com/estree/estree
  // https://docs.python.org/3/library/ast.html
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
}

export interface BinaryExprNode extends UNodeBase {
  kind: "BinaryExpr";
  operator: string;
  left: UNode;
  right: UNode;
}

export interface UnaryExprNode extends UNodeBase {
  kind: "UnaryExpr";
  operator: string;
  operand: UNode;
}

export interface AssignExprNode extends UNodeBase {
  kind: "AssignExpr";
  operator: string;
  target: UNode;
  value: UNode;
  // Citation: assignment expression/statement coverage across Python, Rust, C++, JS.
  // https://docs.python.org/3/library/ast.html
  // https://docs.rs/syn/latest/syn/
  // https://clang.llvm.org/docs/IntroductionToTheClangAST.html
  // https://github.com/estree/estree
}

export interface ImportDeclNode extends UNodeBase {
  kind: "ImportDecl";
  source: string;
  specifiers: Array<{
    imported: string;
    local?: string;
    isTypeOnly?: boolean;
  }>;
  // Citation: covers Python Import/ImportFrom and ESTree ImportDeclaration forms;
  // type-only imports reflect TypeScript ecosystem practice.
  // https://docs.python.org/3/library/ast.html
  // https://github.com/estree/estree
  // https://typescript-eslint.io/packages/typescript-estree/
}

export interface ExportDeclNode extends UNodeBase {
  kind: "ExportDecl";
  exported: UNode[];
  source?: string;
  // Citation: ESTree export model is a dominant Node.js tooling standard;
  // re-export source follows ES module AST conventions.
  // https://github.com/estree/estree
}

export interface CommentNode extends UNodeBase {
  kind: "Comment";
  text: string;
  commentKind: "line" | "block" | "doc";
  // Citation: comments/trivia are often tracked out-of-band in parsers;
  // preserving them is industry practice for safe codemods and formatting.
  // ESTree tooling and Tree-sitter ecosystems both rely on retained ranges/comments.
  // https://github.com/estree/estree
  // https://tree-sitter.github.io/tree-sitter/
}

export type UNode =
  | FileNode
  | ImportDeclNode
  | ExportDeclNode
  | TypeDeclNode
  | FunctionDeclNode
  | MethodDeclNode
  | ParamNode
  | BlockStmtNode
  | VarDeclNode
  | IfStmtNode
  | MatchStmtNode
  | ReturnStmtNode
  | AssignExprNode
  | BinaryExprNode
  | UnaryExprNode
  | CallExprNode
  | MemberExprNode
  | IdentifierNode
  | LiteralNode
  | TypeRefNode
  | CommentNode;
```

## Parser Provider Contract

```ts
/**
 * Native AST provider contract.
 * Industry-compatible parser outputs are first-class artifacts.
 */
export interface AstProvider {
  language: Language;
  parseToNative(source: string, file: string): unknown;
  mapNativeToUast(nativeRoot: unknown, file: string): FileNode;
}

/**
 * Recommended default providers by language:
 *
 * python:
 *   CPython ast
 *   Citation: https://docs.python.org/3/library/ast.html
 *
 * rust:
 *   syn crate (widely used in Rust tooling/proc-macro ecosystem)
 *   Citation: https://docs.rs/syn/latest/syn/
 *
 * cpp:
 *   Clang AST via libclang/clang tooling
 *   Citation: https://clang.llvm.org/docs/IntroductionToTheClangAST.html
 *
 * javascript:
 *   ESTree-compatible parser output
 *   Citation: https://github.com/estree/estree
 *
 * typescript:
 *   typescript-estree (ESTree bridge) or TS compiler AST mapped to ESTree/UAST
 *   Citation: https://typescript-eslint.io/packages/typescript-estree/
 */
export const providers: Record<Language, AstProvider> = {} as any;
```

## Tree-sitter Integration Notes

```ts
/**
 * Tree-sitter returns CSTs (concrete syntax trees), not a canonical semantic AST.
 * Citation: https://tree-sitter.github.io/tree-sitter/
 *
 * Industry-aligned approach:
 * 1) Use native parser AST as the canonical "industry" shape where possible.
 * 2) Use Tree-sitter for fast incremental parsing and source anchoring.
 * 3) Map CST -> UAST (or CST -> native-like AST -> UAST) with explicit field rules.
 */
```

### Standard Tree-sitter mapping rules

- Use named nodes as AST candidates.
- Ignore punctuation-only nodes unless semantically meaningful.
- Use grammar field names to map semantic roles.
- Preserve exact byte/line spans on all UAST nodes.
- Preserve lossless native references in `native`.

## Conformance Checklist

- For each language parser, maintain a golden corpus of files.
- Compare native parser output and mapped UAST for core constructs:
  - declarations (types, functions, methods),
  - control flow (`if`, loops, `match`/`switch`),
  - call/member/assignment expressions,
  - imports/exports/modules.
- Require stable span and provenance for every UAST node.

## Why this matches industry practice

- It keeps native parser ASTs as source of truth (matches ecosystem expectations).
- It adds a normalized layer for cross-language analysis (common in multi-language tooling).
- It preserves location and parser provenance for diagnostics and automated edits.

